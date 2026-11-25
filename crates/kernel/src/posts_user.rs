//! 用户-岗位绑定接口。
//!
//! 边界：岗位 CRUD 在 `posts`；本模块只处理用户绑定。
//! 关键约束：岗位与用户都必须属于当前租户。

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::posts_types::PostRequest;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::tenant::TenantTx;

/// 用户岗位路由（挂在根路径下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route(
        "/api/system/users/{id}/posts",
        get(get_user_posts).put(set_user_posts),
    )
}

/// 用户岗位绑定请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserPostRequest {
    /// 岗位 id 列表（全量覆盖）。
    pub post_ids: Vec<Uuid>,
}

/// 查询用户已绑定岗位。
#[utoipa::path(
    get,
    path = "/api/system/users/{id}/posts",
    tag = "system",
    params(("id" = Uuid, Path, description = "用户 id")),
    responses((status = 200, description = "岗位 id 列表", body = ApiResponse<Vec<Uuid>>))
)]
async fn get_user_posts(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(user_id): Path<Uuid>,
) -> Result<ApiResponse<Vec<Uuid>>, AppError> {
    require_perm(&user, crate::perms::USER_LIST)?;
    ensure_user_in_tenant(&mut tx, user.tenant_id, user_id).await?;

    let ids = sqlx::query_scalar("SELECT post_id FROM sys.sys_user_post WHERE user_id = $1")
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(ids))
}

/// 全量覆盖用户岗位。
#[utoipa::path(
    put,
    path = "/api/system/users/{id}/posts",
    tag = "system",
    params(("id" = Uuid, Path, description = "用户 id")),
    request_body = UserPostRequest,
    responses((status = 200, description = "保存后的岗位 id 列表", body = ApiResponse<Vec<Uuid>>))
)]
async fn set_user_posts(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(user_id): Path<Uuid>,
    Json(req): Json<UserPostRequest>,
) -> Result<ApiResponse<Vec<Uuid>>, AppError> {
    require_perm(&user, crate::perms::USER_EDIT)?;
    ensure_user_in_tenant(&mut tx, user.tenant_id, user_id).await?;

    // 校验岗位都属于当前租户，防止跨租户绑定
    for post_id in &req.post_ids {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sys.sys_post WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL)",
        )
        .bind(post_id)
        .bind(user.tenant_id)
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            return Err(AppError::NotFound);
        }
    }

    sqlx::query("DELETE FROM sys.sys_user_post WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    for post_id in &req.post_ids {
        sqlx::query("INSERT INTO sys.sys_user_post (user_id, post_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(user_id)
            .bind(post_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.user.posts.set",
        "sys_user_post",
        Some(&user_id.to_string()),
        Some(serde_json::json!({ "postIds": req.post_ids })),
    )
    .await;

    Ok(ApiResponse::new(req.post_ids))
}

pub(crate) fn require_perm(user: &CurrentUser, code: &str) -> Result<(), AppError> {
    if user.has_permission(code) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

pub(crate) fn validate_post(req: &PostRequest) -> Result<(), AppError> {
    let code = req.code.trim();
    if code.is_empty() || code.len() > 64 {
        return Err(AppError::Validation(
            "岗位编码长度必须为 1~64 个字符".into(),
        ));
    }
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(AppError::Validation(
            "岗位名称长度必须为 1~64 个字符".into(),
        ));
    }
    let status = req.status.as_deref().unwrap_or("active");
    if !matches!(status, "active" | "disabled") {
        return Err(AppError::Validation("非法的岗位状态".into()));
    }
    Ok(())
}

async fn ensure_user_in_tenant(
    conn: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sys.sys_user WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL)",
    )
    .bind(user_id)
    .bind(tenant_id)
    .fetch_one(&mut *conn)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}
