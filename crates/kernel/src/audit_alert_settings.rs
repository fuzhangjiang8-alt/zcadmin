//! 审计告警配置与 Webhook 设置。
//!
//! 边界：告警规则在 `audit_rules`；告警记录在 `audit_alerts`。
//! 关键约束：Webhook 更新需校验 URL；secret 回显只返回是否已配置。

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;

/// 告警设置路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/audit-alert-config",
            get(get_alert_config).put(update_alert_config),
        )
        .route(
            "/audit-webhook",
            get(get_audit_webhook).put(update_audit_webhook),
        )
}

/// 审计告警配置。
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertConfigVo {
    /// 统计窗口（分钟，1~60）。
    pub window_minutes: i32,
    /// 触发阈值（次数，1~10000）。
    pub threshold: i32,
}

/// 查询审计告警配置。
#[utoipa::path(
    get,
    path = "/api/system/audit-alert-config",
    tag = "system",
    responses((status = 200, description = "告警配置", body = ApiResponse<AlertConfigVo>))
)]
async fn get_alert_config(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<AlertConfigVo>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let row: Option<(i32, i32)> = sqlx::query_as(
        "SELECT window_minutes, threshold FROM sys.sys_audit_alert_rule WHERE tenant_id = $1 AND action_pattern = '*'",
    )
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (window_minutes, threshold) = row.unwrap_or((5, 50));
    tx.commit().await?;
    Ok(ApiResponse::new(AlertConfigVo {
        window_minutes,
        threshold,
    }))
}

/// 更新审计告警配置。
#[utoipa::path(
    put,
    path = "/api/system/audit-alert-config",
    tag = "system",
    request_body = AlertConfigVo,
    responses((status = 200, description = "保存后的配置", body = ApiResponse<AlertConfigVo>))
)]
async fn update_alert_config(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<AlertConfigVo>,
) -> Result<ApiResponse<AlertConfigVo>, AppError> {
    require_perm(&user, crate::perms::AUDIT_ALERT)?;
    if !(1..=60).contains(&req.window_minutes) {
        return Err(AppError::Validation("窗口需在 1~60 分钟".into()));
    }
    if !(1..=10000).contains(&req.threshold) {
        return Err(AppError::Validation("阈值需在 1~10000".into()));
    }
    sqlx::query(
        r#"
        INSERT INTO sys.sys_audit_alert_rule
            (id, tenant_id, action_pattern, window_minutes, threshold, enabled, updated_at)
        VALUES (gen_random_uuid(), $1, '*', $2, $3, true, now())
        ON CONFLICT (tenant_id, action_pattern) DO UPDATE
        SET window_minutes = EXCLUDED.window_minutes,
            threshold = EXCLUDED.threshold,
            enabled = true,
            updated_at = now()
        "#,
    )
    .bind(user.tenant_id)
    .bind(req.window_minutes)
    .bind(req.threshold)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.audit.alert_config",
        "audit_alert_config",
        None,
        Some(serde_json::json!({
            "windowMinutes": req.window_minutes,
            "threshold": req.threshold
        })),
    )
    .await;

    Ok(ApiResponse::new(req))
}

/// 审计 Webhook 配置（secret 只返回是否已设置）。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WebhookVo {
    /// 回调地址。
    pub url: String,
    /// 是否启用。
    pub enabled: bool,
    /// 是否已配置签名密钥。
    pub has_secret: bool,
    /// 自定义 payload 模板（空表示默认 JSON）。
    pub payload_template: Option<String>,
}

/// 更新 Webhook 请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WebhookRequest {
    /// 回调地址。
    pub url: String,
    /// 签名密钥（不传表示保持原值）。
    pub secret: Option<String>,
    /// 是否启用。
    pub enabled: bool,
    /// 自定义 payload 模板（不传表示保持原值）。
    pub payload_template: Option<String>,
}

/// 查询审计 Webhook。
#[utoipa::path(
    get,
    path = "/api/system/audit-webhook",
    tag = "system",
    responses((status = 200, description = "Webhook 配置", body = ApiResponse<WebhookVo>))
)]
async fn get_audit_webhook(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<WebhookVo>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let row: Option<(String, Option<String>, bool, Option<String>)> = sqlx::query_as(
        "SELECT url, secret, enabled, payload_template FROM sys.sys_audit_webhook WHERE tenant_id = $1",
    )
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (url, secret, enabled, payload_template) = row.unwrap_or_default();
    tx.commit().await?;
    Ok(ApiResponse::new(WebhookVo {
        url,
        enabled,
        has_secret: secret.map(|s| !s.is_empty()).unwrap_or(false),
        payload_template,
    }))
}

/// 更新审计 Webhook。
#[utoipa::path(
    put,
    path = "/api/system/audit-webhook",
    tag = "system",
    request_body = WebhookRequest,
    responses((status = 200, description = "保存后的配置", body = ApiResponse<WebhookVo>))
)]
async fn update_audit_webhook(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<WebhookRequest>,
) -> Result<ApiResponse<WebhookVo>, AppError> {
    require_perm(&user, crate::perms::AUDIT_ALERT)?;
    if req.enabled || !req.url.trim().is_empty() {
        // 带域名解析的校验：只查字面量 IP 会放过「解析到内网的公网域名」。
        crate::ssrf::validate_webhook_url_async(&req.url)
            .await
            .map_err(AppError::Validation)?;
    }
    let (has_secret, payload_template): (bool, Option<String>) = sqlx::query_as(
        r#"
        INSERT INTO sys.sys_audit_webhook
            (tenant_id, url, secret, enabled, payload_template, updated_at, updated_by)
        VALUES ($1, $2, $3, $4, $5, now(), $6)
        ON CONFLICT (tenant_id) DO UPDATE
        SET url = EXCLUDED.url,
            secret = COALESCE(EXCLUDED.secret, sys.sys_audit_webhook.secret),
            enabled = EXCLUDED.enabled,
            payload_template = COALESCE(EXCLUDED.payload_template, sys.sys_audit_webhook.payload_template),
            updated_at = now(),
            updated_by = EXCLUDED.updated_by
        RETURNING (secret IS NOT NULL AND secret <> ''), payload_template
        "#,
    )
    .bind(user.tenant_id)
    .bind(&req.url)
    .bind(&req.secret)
    .bind(req.enabled)
    .bind(&req.payload_template)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.audit.webhook",
        "audit_webhook",
        None,
        Some(serde_json::json!({
            "url": req.url,
            "enabled": req.enabled,
            "secretChanged": req.secret.is_some()
        })),
    )
    .await;

    Ok(ApiResponse::new(WebhookVo {
        url: req.url,
        enabled: req.enabled,
        has_secret,
        payload_template,
    }))
}
