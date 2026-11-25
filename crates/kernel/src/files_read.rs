//! 文件详情、下载与删除。
//!
//! 边界：上传/列表在 `files`；本模块处理单文件读取与删除。
//! 关键约束：下载校验租户；删除必须先提交元数据软删，再删物理文件。

use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::files::FileVo;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::tenant::TenantTx;

/// 文件读取路由（挂在根路径下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/system/files/{id}",
            axum::routing::get(get_meta).delete(delete_file),
        )
        .route(
            "/api/system/files/{id}/content",
            axum::routing::get(download),
        )
}

/// 文件元数据。
#[utoipa::path(
    get,
    path = "/api/system/files/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "文件 id")),
    responses((status = 200, description = "文件元数据", body = ApiResponse<FileVo>))
)]
async fn get_meta(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<FileVo>, AppError> {
    let vo = sqlx::query_as::<_, FileVo>(
        r#"
        SELECT id, name, mime, size_bytes, created_at
        FROM sys.sys_file
        WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL
          AND (uploader_id = $3 OR $4::bool)
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(user.has_permission(crate::perms::FILE_LIST))
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    tx.commit().await?;
    Ok(ApiResponse::new(vo))
}

/// 下载文件内容。
#[utoipa::path(
    get,
    path = "/api/system/files/{id}/content",
    tag = "system",
    params(("id" = Uuid, Path, description = "文件 id")),
    responses((status = 200, description = "文件字节流"))
)]
async fn download(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let (name, mime, key) = sqlx::query_as::<_, (String, String, String)>(
        "SELECT name, mime, storage_key FROM sys.sys_file
         WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL
           AND (uploader_id = $3 OR $4::bool)",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(user.has_permission(crate::perms::FILE_LIST))
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    // 权限与元数据读取完成后先释放事务，磁盘读取不应长期占用数据库连接。
    tx.commit().await?;
    let bytes = state.storage.load(&key).await.map_err(AppError::Internal)?;

    let mut resp = (StatusCode::OK, bytes).into_response();
    if let Ok(v) = HeaderValue::from_str(&mime) {
        resp.headers_mut().insert(header::CONTENT_TYPE, v);
    }
    if let Ok(v) = HeaderValue::from_str(&content_disposition(&name)) {
        resp.headers_mut().insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(resp)
}

/// 删除文件：先提交元数据软删，再删除物理文件；提交失败时保留原文件。
#[utoipa::path(
    delete,
    path = "/api/system/files/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "文件 id")),
    responses((status = 200, description = "删除成功"))
)]
async fn delete_file(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::FILE_DELETE)?;

    let key: Option<String> = sqlx::query_scalar(
        "UPDATE sys.sys_file SET deleted_at = now() WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL RETURNING storage_key",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;

    let key = key.ok_or(AppError::NotFound)?;

    // 数据库提交失败必须保留文件；反过来的顺序会让仍可见的元数据指向丢失内容。
    tx.commit().await?;
    // 物理删除失败保留软删记录与 storage_key，供运维重试清理，不恢复对外可见性。
    if let Err(e) = state.storage.remove(&key).await {
        tracing::warn!(error = %e, key = %key, "删除物理文件失败，等待清理任务处理");
    }

    crate::audit::record_user(
        &state.db,
        &user,
        "system.file.delete",
        "sys_file",
        Some(&id.to_string()),
        None,
    )
    .await;

    Ok(ApiResponse::new(()))
}

/// 权限校验（与 system 模块保持同一语义）。
pub(crate) fn require_perm(user: &CurrentUser, code: &str) -> Result<(), AppError> {
    if user.has_permission(code) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// 文件名编码为 RFC 5987 的 `filename*`，兼容中文与空格。
pub(crate) fn content_disposition(name: &str) -> String {
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!("attachment; filename*=UTF-8''{encoded}")
}

/// 审计：上传动作单独记录（文件名可能敏感，只记 id 与大小）。
pub(crate) async fn kernel_audit(state: &AppState, user: &CurrentUser, vo: &FileVo) {
    crate::audit::record_user(
        &state.db,
        user,
        "system.file.upload",
        "sys_file",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "sizeBytes": vo.size_bytes, "mime": vo.mime })),
    )
    .await;
}
