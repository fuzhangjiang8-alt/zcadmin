//! 在线会话管理接口：列表与强制下线。
//!
//! 边界：会话存储在 `session`；本模块只暴露管理接口。
//! 关键约束：只能操作当前租户的会话。

use axum::extract::{Path, State};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;

/// 会话管理路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/sessions", get(list_sessions))
        .route("/sessions/{id}", axum::routing::delete(revoke_session))
}

/// 在线会话视图。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionVo {
    /// 会话 id（非令牌）。
    pub id: Uuid,
    /// 用户 id。
    pub user_id: Uuid,
    /// 登录名。
    pub username: String,
    /// 部门 id。
    pub dept_id: Option<Uuid>,
    /// 登录 IP。
    pub ip: Option<String>,
    /// 登录 User-Agent。
    pub user_agent: Option<String>,
    /// 登录时间。
    pub created_at: DateTime<Utc>,
    /// 过期时间。
    pub expires_at: DateTime<Utc>,
}

/// 在线会话列表（当前租户）。
#[utoipa::path(
    get,
    path = "/api/system/sessions",
    tag = "system",
    responses((status = 200, description = "在线会话", body = ApiResponse<Vec<SessionVo>>))
)]
async fn list_sessions(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<ApiResponse<Vec<SessionVo>>, AppError> {
    require_perm(&user, crate::perms::SESSION_LIST)?;
    let rows = state
        .sessions
        .list(user.tenant_id)
        .await
        .map_err(AppError::Database)?
        .into_iter()
        .map(|s| SessionVo {
            id: s.id,
            user_id: s.user_id,
            username: s.username,
            dept_id: s.dept_id,
            ip: s.ip,
            user_agent: s.user_agent,
            created_at: s.created_at,
            expires_at: s.expires_at,
        })
        .collect();
    Ok(ApiResponse::new(rows))
}

/// 强制下线指定会话。
#[utoipa::path(
    delete,
    path = "/api/system/sessions/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "会话 id")),
    responses((status = 200, description = "已下线"))
)]
async fn revoke_session(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::SESSION_REVOKE)?;
    if !state
        .sessions
        .revoke(user.tenant_id, id)
        .await
        .map_err(AppError::Database)?
    {
        return Err(AppError::NotFound);
    }
    crate::audit::record_user(
        &state.db,
        &user,
        "system.session.revoke",
        "session",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}
