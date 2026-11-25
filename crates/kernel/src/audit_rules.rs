//! 审计告警规则：列表、全量替换与预览。
//!
//! 边界：告警配置/Webhook 在 `audit_alert_settings`；告警记录在 `audit_alerts`。
//! 关键约束：规则替换为全量覆盖，在单事务内完成。

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;

/// 告警规则路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/audit-alert-rules",
            get(list_audit_rules).put(replace_audit_rules),
        )
        .route(
            "/audit-alert-rules/preview",
            axum::routing::post(preview_audit_rule),
        )
}

/// 告警规则。
#[derive(Debug, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditAlertRuleVo {
    /// 规则 id。
    pub id: Uuid,
    /// 动作模式（SQL LIKE，`*` 表示全部）。
    pub action_pattern: String,
    /// 窗口（分钟）。
    pub window_minutes: i32,
    /// 阈值。
    pub threshold: i32,
    /// 静默期（分钟，0 表示不静默）。
    pub silence_minutes: i32,
    /// 是否启用。
    pub enabled: bool,
}

/// 规则列表。
#[utoipa::path(
    get,
    path = "/api/system/audit-alert-rules",
    tag = "system",
    responses((status = 200, description = "规则列表", body = ApiResponse<Vec<AuditAlertRuleVo>>))
)]
async fn list_audit_rules(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<Vec<AuditAlertRuleVo>>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let rows: Vec<AuditAlertRuleVo> = sqlx::query_as(
        r#"
        SELECT id, action_pattern, window_minutes, threshold, silence_minutes, enabled
        FROM sys.sys_audit_alert_rule
        WHERE tenant_id = $1
        ORDER BY length(action_pattern) DESC, action_pattern
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 告警规则输入（不含 id）。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditAlertRuleInput {
    /// 动作模式（SQL LIKE，`*` 表示全部）。
    pub action_pattern: String,
    /// 窗口（分钟）。
    pub window_minutes: i32,
    /// 阈值。
    pub threshold: i32,
    /// 静默期（分钟，0 表示不静默）。
    pub silence_minutes: i32,
    /// 是否启用。
    pub enabled: bool,
}

/// 全量替换告警规则请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceRulesRequest {
    /// 规则列表（至少包含 `*`）。
    pub rules: Vec<AuditAlertRuleInput>,
}

/// 全量替换告警规则。
#[utoipa::path(
    put,
    path = "/api/system/audit-alert-rules",
    tag = "system",
    request_body = ReplaceRulesRequest,
    responses((status = 200, description = "保存后的规则", body = ApiResponse<Vec<AuditAlertRuleVo>>))
)]
async fn replace_audit_rules(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<ReplaceRulesRequest>,
) -> Result<ApiResponse<Vec<AuditAlertRuleVo>>, AppError> {
    require_perm(&user, crate::perms::AUDIT_ALERT)?;
    if !req.rules.iter().any(|r| r.action_pattern == "*") {
        return Err(AppError::Validation("规则必须包含 * 通配项".into()));
    }
    let mut seen = std::collections::HashSet::new();
    for rule in &req.rules {
        if !seen.insert(rule.action_pattern.as_str()) {
            return Err(AppError::Validation(format!(
                "规则重复：{}",
                rule.action_pattern
            )));
        }
        if !(1..=60).contains(&rule.window_minutes)
            || !(1..=10000).contains(&rule.threshold)
            || !(0..=1440).contains(&rule.silence_minutes)
        {
            return Err(AppError::Validation("窗口、阈值或静默期超出范围".into()));
        }
    }
    sqlx::query("DELETE FROM sys.sys_audit_alert_rule WHERE tenant_id = $1")
        .bind(user.tenant_id)
        .execute(&mut *tx)
        .await?;
    for rule in &req.rules {
        sqlx::query(
            "INSERT INTO sys.sys_audit_alert_rule
                (id, tenant_id, action_pattern, window_minutes, threshold, silence_minutes, enabled, updated_at)
             VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, now())
             ON CONFLICT (tenant_id, action_pattern) DO UPDATE
             SET window_minutes = EXCLUDED.window_minutes,
                 threshold = EXCLUDED.threshold,
                 silence_minutes = EXCLUDED.silence_minutes,
                 enabled = EXCLUDED.enabled,
                 updated_at = now()",
        )
        .bind(user.tenant_id)
        .bind(&rule.action_pattern)
        .bind(rule.window_minutes)
        .bind(rule.threshold)
        .bind(rule.silence_minutes)
        .bind(rule.enabled)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.audit.alert_rules",
        "audit_alert_rule",
        None,
        Some(serde_json::json!({ "count": req.rules.len() })),
    )
    .await;

    let (_, body) = {
        let mut tx = TenantTx::begin_tenant(&state.db, user.tenant_id).await?;
        let rows: Vec<AuditAlertRuleVo> = sqlx::query_as(
            r#"
            SELECT id, action_pattern, window_minutes, threshold, silence_minutes, enabled
            FROM sys.sys_audit_alert_rule
            WHERE tenant_id = $1
            ORDER BY length(action_pattern) DESC, action_pattern
            "#,
        )
        .bind(user.tenant_id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        ((), ApiResponse::new(rows))
    };
    Ok(body)
}

/// 规则测试请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RulePreviewRequest {
    /// 动作。
    pub action: String,
    /// 模拟次数。
    pub count: i64,
}

/// 规则测试结果。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RulePreviewVo {
    /// 命中的规则模式。
    pub matched_pattern: String,
    /// 窗口（分钟）。
    pub window_minutes: i32,
    /// 阈值。
    pub threshold: i32,
    /// 静默期（分钟）。
    pub silence_minutes: i32,
    /// 是否会触发告警。
    pub would_alert: bool,
    /// 原因说明。
    pub reason: String,
}

/// 测试某动作在指定次数下是否会触发告警。
#[utoipa::path(
    post,
    path = "/api/system/audit-alert-rules/preview",
    tag = "system",
    request_body = RulePreviewRequest,
    responses((status = 200, description = "测试结果", body = ApiResponse<RulePreviewVo>))
)]
async fn preview_audit_rule(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<RulePreviewRequest>,
) -> Result<ApiResponse<RulePreviewVo>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let rule: Option<(String, i32, i32, i32)> = sqlx::query_as(
        r#"
        SELECT action_pattern, window_minutes, threshold, silence_minutes
        FROM sys.sys_audit_alert_rule
        WHERE tenant_id = $1 AND enabled
          AND $2 LIKE replace(action_pattern, '*', '%')
        ORDER BY length(action_pattern) DESC
        LIMIT 1
        "#,
    )
    .bind(user.tenant_id)
    .bind(&req.action)
    .fetch_optional(&mut *tx)
    .await?;
    let (pattern, window_minutes, threshold, silence_minutes) =
        rule.unwrap_or(("*".to_string(), 5, 50, 30));

    let silenced: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sys.sys_audit_alert
         WHERE tenant_id = $1 AND action = $2
           AND created_at >= now() - make_interval(mins => $3)",
    )
    .bind(user.tenant_id)
    .bind(&req.action)
    .bind(silence_minutes)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    let (would_alert, reason) = if silenced > 0 {
        (false, "静默期内已有告警".to_string())
    } else if req.count >= threshold as i64 {
        (true, format!("达到阈值（{} >= {}）", req.count, threshold))
    } else {
        (
            false,
            format!("未达到阈值（{} < {}）", req.count, threshold),
        )
    };

    Ok(ApiResponse::new(RulePreviewVo {
        matched_pattern: pattern,
        window_minutes,
        threshold,
        silence_minutes,
        would_alert,
        reason,
    }))
}
