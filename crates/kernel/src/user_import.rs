//! 用户批量导入（Excel）。
//!
//! # 为什么限制 500 行
//!
//! 每行都要做一次 Argon2id 哈希（约 40~60ms），1000 行就会让请求超过 60 秒。
//! 500 行是「一次能讲清楚错误」的合理批量；更大规模应走异步任务。
//!
//! # 列约定
//!
//! | 列 | 必填 | 说明 |
//! |---|---|---|
//! | 用户名 | ✅ | 租户内唯一，3~64 字符 |
//! | 显示名 | ✅ | |
//! | 初始密码 | ✅ | 必须满足密码强度 |
//! | 邮箱 | | |
//! | 部门 | | 按部门名称匹配，不存在则该行失败 |
//!
//! 事务边界：每行使用保存点隔离数据库错误，成功行在整批结束后一次提交。
//! Excel 生成与解析均走受控阻塞池，错误报告最多 500 行且不包含初始密码。
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::user_import_parse::{
    insert_row, parse_rows, validate_row, ImportErrorsRequest, ImportOptions, UserImportError,
    UserImportResult,
};

use crate::state::AppState;
use crate::tenant::TenantTx;
use axum::extract::{DefaultBodyLimit, Json, Multipart, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use sqlx::Acquire;

/// 单次导入上限（Argon2 成本决定）。
const MAX_ROWS: usize = 500;

/// 单个导入文件的体积上限。
///
/// 500 行的 xlsx 通常只有几十 KB；给到 10MB 是为了容忍格式冗余，
/// 同时避免"传一个 2GB 文件把进程撑爆"。
const IMPORT_MAX_BYTES: usize = 10 * 1024 * 1024;

/// 路由。
pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/system/users/import",
            post(import_users).layer(DefaultBodyLimit::max(IMPORT_MAX_BYTES)),
        )
        .route("/api/system/users/import-template", get(import_template))
        .route(
            "/api/system/users/import-errors",
            post(export_import_errors),
        )
}

/// 下载用户导入模板；生成工作簿走受控阻塞池，过载时返回 503。
#[utoipa::path(
    get,
    path = "/api/system/users/import-template",
    tag = "system",
    responses((status = 200, description = "xlsx 模板"))
)]
async fn import_template(user: CurrentUser) -> Result<Response, AppError> {
    if !user.has_permission(crate::perms::USER_ADD) {
        return Err(AppError::Forbidden);
    }
    let bytes = crate::xlsx::run(crate::user_import_xlsx::build_template).await?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"user-import-template.xlsx\"",
            ),
        ],
        bytes,
    )
        .into_response())
}

/// 导出最多 500 条错误行；只导出非密码列，生成工作簿走受控阻塞池。
#[utoipa::path(
    post,
    path = "/api/system/users/import-errors",
    tag = "system",
    request_body = ImportErrorsRequest,
    responses((status = 200, description = "xlsx 错误报告"))
)]
async fn export_import_errors(
    user: CurrentUser,
    Json(req): Json<ImportErrorsRequest>,
) -> Result<Response, AppError> {
    if !user.has_permission(crate::perms::USER_ADD) {
        return Err(AppError::Forbidden);
    }
    if req.errors.len() > MAX_ROWS {
        return Err(AppError::Validation(format!("错误报告最多 {MAX_ROWS} 行")));
    }
    let bytes = crate::xlsx::run(move || crate::user_import_xlsx::build_errors(req.errors)).await?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"user-import-errors.xlsx\"",
            ),
        ],
        bytes,
    )
        .into_response())
}

/// 导入用户：行级保存点隔离约束失败，成功行最终一次提交；预校验不写用户数据。
#[utoipa::path(
    post,
    path = "/api/system/users/import",
    tag = "system",
    responses((status = 200, description = "导入结果", body = ApiResponse<UserImportResult>))
)]
async fn import_users(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(options): Query<ImportOptions>,
    mut multipart: Multipart,
) -> Result<ApiResponse<UserImportResult>, AppError> {
    if !user.has_permission(crate::perms::USER_ADD) {
        return Err(AppError::Forbidden);
    }

    let mut bytes: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Validation(format!("解析上传内容失败: {e}")))?
    {
        if field.file_name().is_none() {
            continue;
        }
        bytes = Some(
            field
                .bytes()
                .await
                .map_err(|e| AppError::Validation(format!("读取文件失败: {e}")))?
                .to_vec(),
        );
        break;
    }
    let bytes = bytes.ok_or_else(|| AppError::Validation("未找到上传文件".into()))?;

    // xlsx 解析是纯 CPU 的同步操作（zip 解压 + XML 扫描），放在阻塞池里跑。
    let rows = crate::blocking::run(move || parse_rows(&bytes)).await??;
    if rows.len() > MAX_ROWS {
        return Err(AppError::Validation(format!(
            "单次最多导入 {MAX_ROWS} 行，当前 {} 行",
            rows.len()
        )));
    }

    let max_users: i32 = sqlx::query_scalar("SELECT max_users FROM sys.sys_tenant WHERE id = $1")
        .bind(user.tenant_id)
        .fetch_one(&mut *tx)
        .await?;
    if max_users > 0 {
        let used: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM sys.sys_user
             WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(user.tenant_id)
        .fetch_one(&mut *tx)
        .await?;
        if used + rows.len() as i64 > i64::from(max_users) {
            return Err(AppError::Validation("导入后用户数将超过租户配额".into()));
        }
    }

    let mut result = UserImportResult {
        total: rows.len(),
        success: 0,
        failed: 0,
        errors: Vec::new(),
    };

    let dry_run = options.dry_run.unwrap_or(false);
    let mut seen_usernames = std::collections::HashSet::new();
    for (idx, row) in rows.iter().enumerate() {
        let username = row.username.trim();
        if !username.is_empty() && !seen_usernames.insert(username.to_string()) {
            result.failed += 1;
            result.errors.push(UserImportError::new(
                idx + 2,
                row,
                "文件内用户名重复".into(),
            ));
            continue;
        }
        // 这是当前事务内的 SAVEPOINT，不是独立提交；数据库约束失败不能污染后续行。
        let mut savepoint = (&mut *tx).begin().await?;
        let outcome = if dry_run {
            validate_row(&mut savepoint, &user, row).await.map(|_| ())
        } else {
            insert_row(&mut savepoint, &user, row).await
        };
        match outcome {
            Ok(()) => {
                savepoint.commit().await?;
                result.success += 1;
            }
            Err(msg) => {
                savepoint.rollback().await?;
                result.failed += 1;
                result.errors.push(UserImportError::new(idx + 2, row, msg));
            }
        }
    }

    tx.commit().await?;

    crate::audit::record_user(
        &_state.db,
        &user,
        if dry_run {
            "system.user.import.preview"
        } else {
            "system.user.import"
        },
        "sys_user",
        None,
        Some(serde_json::json!({
            "total": result.total, "success": result.success, "failed": result.failed
        })),
    )
    .await;

    Ok(ApiResponse::new(result))
}

/// OpenAPI 片段。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(import_users, import_template, export_import_errors),
    components(schemas(UserImportResult, UserImportError, ImportErrorsRequest)),
    tags((name = "system", description = "系统管理：用户/角色/权限/菜单/字典"))
)]
pub struct UserImportApi;
