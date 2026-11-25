//! 密码策略读写（长度 / 复杂度 / 历史数量 / 有效期）。
//!
//! # 边界
//!
//! - 密码哈希与校验在 `password`；登录失败锁定与风控看板在 `system_login_security`；
//! - 本模块只维护「策略数据」本身，不参与登录流程判定。
//!
//! # 关键约束
//!
//! 策略按租户隔离（`sys_password_policy.tenant_id`）；读写分别要求
//! `system:config:list` 与 `system:config:edit`。未配置时返回默认值而不是报错，
//! 避免新租户一登录就看不到策略。

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// 密码策略路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route(
        "/password-policy",
        get(get_password_policy).put(update_password_policy),
    )
}

/// 密码策略视图。
#[derive(Debug, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordPolicyVo {
    /// 最小长度。
    pub min_length: i32,
    /// 要求大写。
    pub require_upper: bool,
    /// 要求小写。
    pub require_lower: bool,
    /// 要求数字。
    pub require_digit: bool,
    /// 历史密码保留数量。
    pub history_count: i32,
    /// 密码有效天数（0 表示不过期）。
    pub expires_days: i32,
}

/// 查询密码策略。
#[utoipa::path(
    get,
    path = "/api/system/password-policy",
    tag = "system",
    responses((status = 200, description = "密码策略", body = ApiResponse<PasswordPolicyVo>))
)]
async fn get_password_policy(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<PasswordPolicyVo>, AppError> {
    require_perm(&user, "system:config:list")?;
    let row: Option<PasswordPolicyVo> = sqlx::query_as(
        "SELECT min_length, require_upper, require_lower, require_digit, history_count,
                password_expires_days AS expires_days
         FROM sys.sys_password_policy WHERE tenant_id = $1",
    )
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(row.unwrap_or(PasswordPolicyVo {
        min_length: 10,
        require_upper: true,
        require_lower: true,
        require_digit: true,
        history_count: 5,
        expires_days: 0,
    })))
}

/// 更新密码策略。
#[utoipa::path(
    put,
    path = "/api/system/password-policy",
    tag = "system",
    request_body = PasswordPolicyVo,
    responses((status = 200, description = "保存后的策略", body = ApiResponse<PasswordPolicyVo>))
)]
async fn update_password_policy(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<PasswordPolicyVo>,
) -> Result<ApiResponse<PasswordPolicyVo>, AppError> {
    require_perm(&user, "system:config:edit")?;
    if !(6..=64).contains(&req.min_length) {
        return Err(AppError::Validation("最小长度必须在 6-64".into()));
    }
    if !(0..=20).contains(&req.history_count) {
        return Err(AppError::Validation("历史数量必须在 0-20".into()));
    }
    if !(0..=3650).contains(&req.expires_days) {
        return Err(AppError::Validation("密码有效天数必须在 0-3650".into()));
    }
    let old: PasswordPolicyVo = sqlx::query_as(
        "SELECT min_length, require_upper, require_lower, require_digit, history_count,
                password_expires_days AS expires_days
         FROM sys.sys_password_policy WHERE tenant_id = $1",
    )
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or(PasswordPolicyVo {
        min_length: 10,
        require_upper: true,
        require_lower: true,
        require_digit: true,
        history_count: 5,
        expires_days: 0,
    });
    sqlx::query(
        "INSERT INTO sys.sys_password_policy
            (tenant_id, min_length, require_upper, require_lower, require_digit,
             history_count, password_expires_days, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now())
         ON CONFLICT (tenant_id) DO UPDATE
         SET min_length = EXCLUDED.min_length,
             require_upper = EXCLUDED.require_upper,
             require_lower = EXCLUDED.require_lower,
             require_digit = EXCLUDED.require_digit,
             history_count = EXCLUDED.history_count,
             password_expires_days = EXCLUDED.password_expires_days,
             updated_at = now()",
    )
    .bind(user.tenant_id)
    .bind(req.min_length)
    .bind(req.require_upper)
    .bind(req.require_lower)
    .bind(req.require_digit)
    .bind(req.history_count)
    .bind(req.expires_days)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    crate::audit::record_with_meta(
        &state.db,
        user.tenant_id,
        Some(user.user_id),
        "system.password_policy.update",
        "sys_password_policy",
        Some(&user.tenant_id.to_string()),
        user.ip.as_deref(),
        user.user_agent.as_deref(),
        Some(serde_json::json!({
            "old": &old,
            "new": &req
        })),
    )
    .await;
    Ok(ApiResponse::new(req))
}
