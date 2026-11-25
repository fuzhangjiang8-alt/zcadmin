//! 登录风控与登录策略、账号解锁接口。
//!
//! 边界：密码策略在 `system_security`；本模块处理登录侧。
//! 关键约束：风控统计按租户隔离；解锁会清空失败计数与锁定时间。

use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::audit_stats::{AlertUserStat, AuditStatsQuery};
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::system_security::{reset_user_password, unlock_user};
use crate::tenant::TenantTx;

/// 登录安全路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/login-risk", get(login_risk))
        .route(
            "/login-policy",
            get(get_login_policy).put(update_login_policy),
        )
        .route("/users/{id}/unlock", post(unlock_user))
        .route("/users/{id}/reset-password", post(reset_user_password))
}

/// 登录风控按天统计。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginDayStat {
    /// 日期。
    pub day: String,
    /// 成功次数。
    pub success: i64,
    /// 失败次数。
    pub failed: i64,
}

/// 登录风控看板。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginRiskVo {
    /// 统计天数。
    pub days: i64,
    /// 成功登录。
    pub success: i64,
    /// 失败登录。
    pub failed: i64,
    /// 账号锁定。
    pub locked: i64,
    /// 新设备告警。
    pub new_devices: i64,
    /// 按天趋势。
    pub by_day: Vec<LoginDayStat>,
    /// 失败最多的用户（Top 5）。
    pub top_failed: Vec<AlertUserStat>,
}

/// 登录风控看板数据。
#[utoipa::path(
    get,
    path = "/api/system/login-risk",
    tag = "system",
    params(("days" = Option<i64>, Query, description = "最近天数，默认 30")),
    responses((status = 200, description = "登录风控", body = ApiResponse<LoginRiskVo>))
)]
async fn login_risk(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<AuditStatsQuery>,
) -> Result<ApiResponse<LoginRiskVo>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let days = q.days.unwrap_or(30).clamp(1, 365);

    let (success, failed, locked): (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            count(*) FILTER (WHERE action = 'auth.login.success')::bigint,
            count(*) FILTER (WHERE action = 'auth.login.failed')::bigint,
            count(*) FILTER (WHERE action = 'auth.login.locked')::bigint
        FROM sys.sys_audit_log
        WHERE tenant_id = $1 AND created_at >= now() - make_interval(days => $2::int)
        "#,
    )
    .bind(user.tenant_id)
    .bind(days as i32)
    .fetch_one(&mut *tx)
    .await?;

    let new_devices: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM sys.sys_audit_alert
         WHERE tenant_id = $1 AND action = 'auth.login.new_device'
           AND created_at >= now() - make_interval(days => $2::int)",
    )
    .bind(user.tenant_id)
    .bind(days as i32)
    .fetch_one(&mut *tx)
    .await?;

    let by_day: Vec<LoginDayStat> = sqlx::query_as(
        r#"
        SELECT to_char(created_at::date, 'YYYY-MM-DD') AS day,
               count(*) FILTER (WHERE action = 'auth.login.success')::bigint AS success,
               count(*) FILTER (WHERE action = 'auth.login.failed')::bigint AS failed
        FROM sys.sys_audit_log
        WHERE tenant_id = $1
          AND created_at >= now() - make_interval(days => $2::int)
          AND action IN ('auth.login.success', 'auth.login.failed')
        GROUP BY created_at::date
        ORDER BY created_at::date
        "#,
    )
    .bind(user.tenant_id)
    .bind(days as i32)
    .fetch_all(&mut *tx)
    .await?;

    let top_failed: Vec<AlertUserStat> = sqlx::query_as(
        r#"
        SELECT u.username, count(*)::bigint AS count
        FROM sys.sys_audit_log a
        JOIN sys.sys_user u ON u.id = a.user_id
        WHERE a.tenant_id = $1
          AND a.action = 'auth.login.failed'
          AND a.created_at >= now() - make_interval(days => $2::int)
        GROUP BY u.username
        ORDER BY count DESC, u.username
        LIMIT 5
        "#,
    )
    .bind(user.tenant_id)
    .bind(days as i32)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(LoginRiskVo {
        days,
        success,
        failed,
        locked,
        new_devices,
        by_day,
        top_failed,
    }))
}

/// 登录风控策略视图。
#[derive(Debug, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginPolicyVo {
    /// 最大失败次数。
    pub max_failed_attempts: i32,
    /// 锁定秒数。
    pub lock_seconds: i64,
    /// 预警百分比。
    pub warning_percent: i32,
}

/// 查询登录风控策略。
#[utoipa::path(
    get,
    path = "/api/system/login-policy",
    tag = "system",
    responses((status = 200, description = "登录策略", body = ApiResponse<LoginPolicyVo>))
)]
async fn get_login_policy(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<LoginPolicyVo>, AppError> {
    require_perm(&user, "system:config:list")?;
    let row: Option<LoginPolicyVo> = sqlx::query_as(
        "SELECT max_failed_attempts, lock_seconds::bigint, warning_percent
         FROM sys.sys_login_policy WHERE tenant_id = $1",
    )
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(row.unwrap_or(LoginPolicyVo {
        max_failed_attempts: 5,
        lock_seconds: 900,
        warning_percent: 60,
    })))
}

/// 更新登录风控策略。
#[utoipa::path(
    put,
    path = "/api/system/login-policy",
    tag = "system",
    request_body = LoginPolicyVo,
    responses((status = 200, description = "保存后的策略", body = ApiResponse<LoginPolicyVo>))
)]
async fn update_login_policy(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<LoginPolicyVo>,
) -> Result<ApiResponse<LoginPolicyVo>, AppError> {
    require_perm(&user, "system:config:edit")?;
    if !(1..=50).contains(&req.max_failed_attempts) {
        return Err(AppError::Validation("最大失败次数必须在 1-50".into()));
    }
    if !(60..=86_400).contains(&req.lock_seconds) {
        return Err(AppError::Validation("锁定秒数必须在 60-86400".into()));
    }
    if !(1..=99).contains(&req.warning_percent) {
        return Err(AppError::Validation("预警百分比必须在 1-99".into()));
    }
    let old: LoginPolicyVo = sqlx::query_as(
        "SELECT max_failed_attempts, lock_seconds::bigint, warning_percent
         FROM sys.sys_login_policy WHERE tenant_id = $1",
    )
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or(LoginPolicyVo {
        max_failed_attempts: 5,
        lock_seconds: 900,
        warning_percent: 60,
    });
    sqlx::query(
        "INSERT INTO sys.sys_login_policy
            (tenant_id, max_failed_attempts, lock_seconds, warning_percent, updated_at)
         VALUES ($1, $2, $3, $4, now())
         ON CONFLICT (tenant_id) DO UPDATE
         SET max_failed_attempts = EXCLUDED.max_failed_attempts,
             lock_seconds = EXCLUDED.lock_seconds,
             warning_percent = EXCLUDED.warning_percent,
             updated_at = now()",
    )
    .bind(user.tenant_id)
    .bind(req.max_failed_attempts)
    .bind(req.lock_seconds as i32)
    .bind(req.warning_percent)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    crate::audit::record_with_meta(
        &state.db,
        user.tenant_id,
        Some(user.user_id),
        "system.login_policy.update",
        "sys_login_policy",
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
