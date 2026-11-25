//! 用户-角色绑定接口（查询与全量覆盖）。
//!
//! 边界：用户主体在 `system_users`，角色主体在 `system_identity`。
//! 关键约束：目标用户与角色都必须属于当前租户；不能修改自己的角色。

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system_identity::{ensure_role_in_tenant, ensure_user_in_tenant, require_perm};
use crate::system_role_permissions::IdListRequest;
use crate::tenant::TenantTx;

/// 用户-角色路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/users/{id}/roles", get(get_user_roles).put(set_user_roles))
}

/// 查询用户已绑定的角色 id。
#[utoipa::path(
    get,
    path = "/api/system/users/{id}/roles",
    tag = "system",
    params(("id" = Uuid, Path, description = "用户 id")),
    responses((status = 200, description = "已绑定的角色 id 列表", body = ApiResponse<Vec<Uuid>>))
)]
async fn get_user_roles(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(user_id): Path<Uuid>,
) -> Result<ApiResponse<Vec<Uuid>>, AppError> {
    require_perm(&user, crate::perms::USER_LIST)?;
    ensure_user_in_tenant(&mut tx, user.tenant_id, user_id).await?;

    let ids =
        sqlx::query_scalar::<_, Uuid>("SELECT role_id FROM sys.sys_user_role WHERE user_id = $1")
            .bind(user_id)
            .fetch_all(&mut *tx)
            .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(ids))
}

/// 全量覆盖用户的角色。
#[utoipa::path(
    put,
    path = "/api/system/users/{id}/roles",
    tag = "system",
    params(("id" = Uuid, Path, description = "用户 id")),
    request_body = IdListRequest,
    responses((status = 200, description = "保存后的角色 id 列表", body = ApiResponse<Vec<Uuid>>))
)]
async fn set_user_roles(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(user_id): Path<Uuid>,
    Json(req): Json<IdListRequest>,
) -> Result<ApiResponse<Vec<Uuid>>, AppError> {
    require_perm(&user, crate::perms::USER_EDIT)?;

    // 目标用户必须属于当前租户（sys_user_role 无 tenant_id，数据库无法兜底）
    ensure_user_in_tenant(&mut tx, user.tenant_id, user_id).await?;

    // 不允许修改自己的角色：避免把自己降权后无法恢复
    if user_id == user.user_id {
        return Err(AppError::Validation("不能修改自己的角色".into()));
    }

    let before: Vec<Uuid> = sqlx::query_scalar(
        "SELECT role_id FROM sys.sys_user_role WHERE user_id = $1 ORDER BY role_id",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM sys.sys_user_role WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    for role_id in &req.ids {
        // 角色也必须属于当前租户，否则会把用户绑定到别租户的角色上
        ensure_role_in_tenant(&mut tx, user.tenant_id, *role_id).await?;
        sqlx::query(
            "INSERT INTO sys.sys_user_role (user_id, role_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(role_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // 角色变化立即生效：会话里缓存的是登录时的权限快照
    match state
        .sessions
        .revoke_user_sessions(user.tenant_id, user_id, None)
        .await
    {
        Ok(n) if n > 0 => {
            tracing::info!(user_id = %user_id, revoked = n, "用户角色变更，已撤销其会话");
        }
        Ok(_) => {}
        Err(e) => tracing::error!(error = %e, user_id = %user_id, "撤销用户会话失败"),
    }

    crate::audit::record_user(
        &state.db,
        &user,
        "system.user.roles",
        "sys_user",
        Some(&user_id.to_string()),
        Some(serde_json::json!({ "before": before, "after": req.ids })),
    )
    .await;

    Ok(ApiResponse::new(req.ids))
}
