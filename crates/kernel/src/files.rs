//! 文件上传与下载。
//!
//! # 流程
//!
//! ```text
//! POST /api/system/files/upload (multipart)
//!   → 校验大小 → 生成 key（{tenant}/{file_id}）→ 写存储 → 写元数据 → 返回 FileVo
//!
//! GET /api/system/files/{id}/content
//!   → 查元数据（租户隔离）→ 读存储 → 返回字节流
//! ```
//!
//! # 为什么不用原始文件名做 key
//!
//! 原始文件名可能包含 `../`、中文、空格、重复名。用服务端生成的 UUID 做 key，
//! 原始文件名只存数据库用于展示与下载时回填。

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::extract::Query;
use crate::files_read::{kernel_audit, require_perm};
use crate::response::{ApiResponse, Page, PageMeta, PageQuery};
use crate::state::AppState;
use crate::tenant::TenantTx;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// 文件列表查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileQuery {
    /// 分页参数。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 文件名关键字（模糊匹配，可选）。
    pub keyword: Option<String>,
}

/// 文件元数据视图对象。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileVo {
    /// 文件 id。
    pub id: Uuid,
    /// 原始文件名。
    pub name: String,
    /// MIME 类型。
    pub mime: String,
    /// 字节数。
    pub size_bytes: i64,
    /// 上传时间。
    pub created_at: DateTime<Utc>,
}

/// 上传路由允许的在途请求数。
///
/// 上传是「25MB 请求体全量读进内存」的操作（`field.bytes()`），
/// 全局并发上限按 CPU×8 算（32 核就是 256）时，峰值内存可达数 GB。
/// 这里单独收口到 4：超额直接 503，让客户端重试，而不是把进程撑爆。
const MAX_CONCURRENT_UPLOADS: usize = 4;

/// 上传并发许可（进程内）。
static UPLOAD_PERMITS: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();

/// 取上传并发许可池（首次调用时按 [`MAX_CONCURRENT_UPLOADS`] 初始化）。
fn upload_permits() -> &'static tokio::sync::Semaphore {
    UPLOAD_PERMITS.get_or_init(|| tokio::sync::Semaphore::new(MAX_CONCURRENT_UPLOADS))
}

/// 文件相关路由。
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/system/files", get(list_files))
        .route(
            "/api/system/files/upload",
            // 上传体积上限：与配置里的 max_file_bytes 保持一致（略大一点留给表单开销）。
            // 只挂在上传路由上：全局 DefaultBodyLimit（2MB）其余接口继续生效，
            // 否则任何接口都能收 25MB 请求体，形成内存放大攻击面。
            // 并发由 handler 内的 [`upload_permits`] 许可池收口（见 `upload_file`）。
            post(upload_file).layer(DefaultBodyLimit::max(25 * 1024 * 1024)),
        )
        .merge(crate::files_read::router())
}

/// 文件列表。
#[utoipa::path(
    get,
    path = "/api/system/files",
    tag = "system",
    params(
        ("page" = Option<u64>, Query, description = "页码"),
        ("size" = Option<u64>, Query, description = "每页条数"),
        ("keyword" = Option<String>, Query, description = "文件名模糊匹配")
    ),
    responses((status = 200, description = "文件分页列表", body = ApiResponse<Page<FileVo>>))
)]
async fn list_files(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<FileQuery>,
) -> Result<ApiResponse<Page<FileVo>>, AppError> {
    require_perm(&user, crate::perms::FILE_LIST)?;
    let kw = q
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{s}%"));

    let rows = sqlx::query_as::<_, FileVo>(
        "SELECT id, name, mime, size_bytes, created_at FROM sys.sys_file
         WHERE tenant_id = $1 AND deleted_at IS NULL AND ($2::text IS NULL OR name ILIKE $2)
         ORDER BY created_at DESC, id LIMIT $3 OFFSET $4",
    )
    .bind(user.tenant_id)
    .bind(kw.as_deref())
    .bind(q.page.limit())
    .bind(q.page.offset())
    .fetch_all(&mut *tx)
    .await?;

    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sys.sys_file
         WHERE tenant_id = $1 AND deleted_at IS NULL AND ($2::text IS NULL OR name ILIKE $2)",
    )
    .bind(user.tenant_id)
    .bind(kw.as_deref())
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(Page {
        data: rows,
        page: PageMeta::new(q.page.page(), q.page.size(), total as u64),
    }))
}

/// 上传文件（multipart/form-data，字段名任意，取第一个带文件名的字段）。
#[utoipa::path(
    post,
    path = "/api/system/files/upload",
    tag = "system",
    responses((status = 200, description = "上传成功", body = ApiResponse<FileVo>))
)]
async fn upload_file(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    mut multipart: Multipart,
) -> Result<ApiResponse<FileVo>, AppError> {
    require_perm(&user, crate::perms::FILE_UPLOAD)?;
    // 先占用并发许可再读请求体：拿不到就快速失败，25MB 的缓冲根本不会分配。
    let _permit = upload_permits().try_acquire().map_err(|_| {
        tracing::warn!(user = %user.username, "上传并发已满，请求被快速拒绝");
        AppError::Unavailable
    })?;

    let max = state.config.storage.max_file_bytes;
    let mut picked: Option<(String, String, Bytes)> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Validation(format!("解析上传内容失败: {e}")))?
    {
        let Some(file_name) = field.file_name().map(|s| s.to_string()) else {
            continue;
        };
        let mime = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();
        let data = field
            .bytes()
            .await
            .map_err(|e| AppError::Validation(format!("读取上传内容失败: {e}")))?;

        if data.len() as u64 > max {
            return Err(AppError::Validation(format!(
                "文件超过大小上限（{} MB）",
                max / 1024 / 1024
            )));
        }
        picked = Some((file_name, mime, data));
        break;
    }

    let (name, mime, data) = picked.ok_or_else(|| AppError::Validation("未找到上传文件".into()))?;

    let max_storage: i64 =
        sqlx::query_scalar("SELECT max_storage_bytes FROM sys.sys_tenant WHERE id = $1")
            .bind(user.tenant_id)
            .fetch_one(&mut *tx)
            .await?;
    let mut quota_used: Option<i64> = None;
    if max_storage > 0 {
        let used: i64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(size_bytes), 0)::bigint FROM sys.sys_file
             WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(user.tenant_id)
        .fetch_one(&mut *tx)
        .await?;
        quota_used = Some(used + data.len() as i64);
        if used + data.len() as i64 > max_storage {
            return Err(AppError::Validation("租户存储配额不足".into()));
        }
    }

    let id = Uuid::now_v7();
    // key 由服务端生成：租户隔离 + UUID，不含任何用户输入
    let key = format!("{}/{}", user.tenant_id, id);

    state
        .storage
        .save(&key, &data)
        .await
        .map_err(AppError::Internal)?;

    let vo = sqlx::query_as::<_, FileVo>(
        r#"
        INSERT INTO sys.sys_file (id, tenant_id, name, mime, size_bytes, storage_key, uploader_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, name, mime, size_bytes, created_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(&name)
    .bind(&mime)
    .bind(data.len() as i64)
    .bind(&key)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    if let Some(used) = quota_used {
        if used * 5 >= max_storage * 4 {
            let _ = crate::audit::record_alert(
                &state.db,
                user.tenant_id,
                "system.tenant.quota.storage",
                used,
                max_storage.min(i32::MAX as i64) as i32,
            )
            .await;
        }
    }
    kernel_audit(&state, &user, &vo).await;
    Ok(ApiResponse::new(vo))
}

/// OpenAPI 片段。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list_files,
        upload_file,
        crate::files_read::get_meta,
        crate::files_read::download,
        crate::files_read::delete_file
    ),
    components(schemas(FileVo)),
    tags((name = "system", description = "系统管理：用户/角色/权限/菜单/字典"))
)]
pub struct FileApi;
