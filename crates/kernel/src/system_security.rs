//! 登录风控、账号解锁与管理员重置密码。
//!
//! # 职责
//!
//! - 管理员解锁账号、重置用户密码并吊销其会话与令牌；
//! - 登录策略（失败锁定阈值/预警比例）读写；
//! - 登录风控看板与登录策略读写。
//!
//! # 边界
//!
//! 认证流程本身在 `auth` 模块；本模块只维护策略数据与风控查询。
//! 所有写接口要求 `system:config:edit` 或 `system:user:edit`。
//!
//! # 关键约束
//!
//! 从 `system.rs` 拆出；密码策略移到 `system_password_policy`；登录风控复用 `audit_stats` 的统计类型。

use crate::audit_stats::{AlertUserStat, AuditStatsQuery};
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;
use axum::extract::{Path, State};

use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

/// 登录风控与账号安全路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .merge(crate::system_login_security::router())
        .merge(crate::system_password_policy::router())
}

/// 解锁账号（清除失败次数与锁定时间）。
#[utoipa::path(
    post,
    path = "/api/system/users/{id}/unlock",
    tag = "system",
    params(("id" = Uuid, Path, description = "用户 id")),
    responses((status = 200, description = "已解锁"))
)]
pub(crate) async fn unlock_user(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, "system:user:edit")?;
    let result = sqlx::query(
        "UPDATE sys.sys_user
         SET failed_attempts = 0, locked_until = NULL, updated_at = now()
         WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user.tenant_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.user.unlock",
        "sys_user",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 管理员重置用户密码请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordRequest {
    /// 新密码（必须满足当前租户密码强度策略）。
    pub password: String,
}

/// 管理员重置指定用户密码（强制该用户下次登录改密并立即吊销全部活跃会话）。
#[utoipa::path(
    post,
    path = "/api/system/users/{id}/reset-password",
    tag = "system",
    params(("id" = Uuid, Path, description = "用户 id")),
    request_body = ResetPasswordRequest,
    responses((status = 200, description = "已重置密码并吊销旧会话"))
)]
pub(crate) async fn reset_user_password(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<ResetPasswordRequest>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, "system:user:edit")?;
    let policy = crate::password::load_policy(&mut tx, user.tenant_id).await?;
    policy
        .validate(&req.password)
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let hash = crate::password::hash_password_async(req.password).await?;
    let affected = sqlx::query(
        "UPDATE sys.sys_user
         SET password_hash = $3, must_change_password = true, failed_attempts = 0,
             locked_until = NULL, password_changed_at = now(), updated_at = now(),
             updated_by = $4, version = version + 1
         WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(hash)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;
    let _ = state
        .sessions
        .revoke_user_sessions(user.tenant_id, id, None)
        .await;
    // 密码轮换必须同时吊销 PAT：令牌是独立凭据，不随会话失效。
    // 少了这一步，「重置密码」只换掉了密码，攻击者用旧令牌照样进来。
    match crate::auth_tokens::revoke_user_tokens(&state.db, user.tenant_id, id).await {
        Ok(n) if n > 0 => {
            tracing::info!(user_id = %id, revoked_tokens = n, "重置密码后已吊销访问令牌");
        }
        Ok(_) => {}
        Err(e) => tracing::error!(error = %e, user_id = %id, "吊销访问令牌失败"),
    }
    crate::audit::record_user(
        &state.db,
        &user,
        "system.user.reset_password",
        "sys_user",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 登录风控与账号安全接口的 OpenAPI 片段。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        crate::system_password_policy::get_password_policy,
        crate::system_password_policy::update_password_policy,
        crate::system_login_security::login_risk,
        crate::system_login_security::get_login_policy,
        crate::system_login_security::update_login_policy,
        unlock_user,
        reset_user_password
    ),
    components(schemas(
        crate::system_password_policy::PasswordPolicyVo,
        ResetPasswordRequest,
        crate::system_login_security::LoginRiskVo,
        crate::system_login_security::LoginDayStat,
        crate::system_login_security::LoginPolicyVo,
        AlertUserStat,
        AuditStatsQuery
    ))
)]
pub struct SecurityApi;
