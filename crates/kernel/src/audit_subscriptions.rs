//! 审计告警订阅：当前用户的订阅模式与订阅 Webhook 配置。
//!
//! 边界：告警记录列表在 `audit_alerts`；规则/投递/配置快照在 `audit_admin` 的其它模块。
//! 关键约束：订阅按「租户 + 用户」隔离；模式做基础合法性校验，且查询时必须转义
//! `%`/`_`（见 [`like_expression`]），否则订阅 `%` 等于订阅全租户告警。

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::tenant::TenantTx;

/// 审计订阅路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/audit-alert-subscriptions",
            get(list_alert_subscriptions).put(replace_alert_subscriptions),
        )
        .route(
            "/audit-alert-subscription-config",
            get(get_subscription_config).put(update_subscription_config),
        )
}

/// 订阅项。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionVo {
    /// 动作模式。
    pub action_pattern: String,
}

/// 订阅设置请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionRequest {
    /// 动作模式列表（全量覆盖）。
    pub patterns: Vec<String>,
}

/// 当前用户的告警订阅。
#[utoipa::path(
    get,
    path = "/api/system/audit-alert-subscriptions",
    tag = "system",
    responses((status = 200, description = "订阅列表", body = ApiResponse<Vec<SubscriptionVo>>))
)]
async fn list_alert_subscriptions(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<Vec<SubscriptionVo>>, AppError> {
    let rows: Vec<SubscriptionVo> = sqlx::query_as(
        "SELECT action_pattern FROM sys.sys_audit_alert_subscription
         WHERE tenant_id = $1 AND user_id = $2 ORDER BY action_pattern",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 替换当前用户的告警订阅。
#[utoipa::path(
    put,
    path = "/api/system/audit-alert-subscriptions",
    tag = "system",
    request_body = SubscriptionRequest,
    responses((status = 200, description = "保存后的订阅", body = ApiResponse<Vec<SubscriptionVo>>))
)]
async fn replace_alert_subscriptions(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<SubscriptionRequest>,
) -> Result<ApiResponse<Vec<SubscriptionVo>>, AppError> {
    for pattern in &req.patterns {
        let p = pattern.trim();
        if p.is_empty() || p.len() > 128 {
            return Err(AppError::Validation(
                "订阅模式不能为空且不超过 128 字符".into(),
            ));
        }
    }
    sqlx::query(
        "DELETE FROM sys.sys_audit_alert_subscription WHERE tenant_id = $1 AND user_id = $2",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await?;
    for pattern in &req.patterns {
        sqlx::query(
            "INSERT INTO sys.sys_audit_alert_subscription (id, tenant_id, user_id, action_pattern)
             VALUES (gen_random_uuid(), $1, $2, $3)
             ON CONFLICT DO NOTHING",
        )
        .bind(user.tenant_id)
        .bind(user.user_id)
        .bind(pattern.trim())
        .execute(&mut *tx)
        .await?;
    }
    let rows: Vec<SubscriptionVo> = sqlx::query_as(
        "SELECT action_pattern FROM sys.sys_audit_alert_subscription
         WHERE tenant_id = $1 AND user_id = $2 ORDER BY action_pattern",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 把用户订阅模式安全地转成 SQL `LIKE` 表达式。
///
/// # 为什么必须转义
///
/// 订阅模式是用户输入。直接拼进 `LIKE` 时，`%` 与 `_` 是 SQL 通配符：
/// 用户把模式设成 `%` 就能匹配**整个租户**的告警（包括别人的登录失败、
/// 配额告警），越过「只看自己订阅」的设计。这里只把业务约定的 `*` 当通配符。
///
/// 用法：`a.action LIKE {表达式} ESCAPE '\'`。
pub(crate) fn like_expression(alias: &str) -> String {
    format!(
        r"replace(replace(replace(replace({alias}.action_pattern,'\','\\'),'%','\%'),'_','\_'),'*','%')"
    )
}

/// 订阅 Webhook 配置。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionConfigVo {
    /// 回调地址。
    pub url: String,
    /// 是否启用。
    pub enabled: bool,
    /// 是否已配置签名密钥。
    pub has_secret: bool,
    /// 自定义 payload 模板。
    pub payload_template: Option<String>,
}

/// 更新订阅 Webhook 配置。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionConfigRequest {
    /// 回调地址。
    pub url: String,
    /// 签名密钥（不传保持原值）。
    pub secret: Option<String>,
    /// 是否启用。
    pub enabled: bool,
    /// 自定义 payload 模板（不传保持原值）。
    pub payload_template: Option<String>,
}

/// 查询当前用户的订阅 Webhook。
#[utoipa::path(
    get,
    path = "/api/system/audit-alert-subscription-config",
    tag = "system",
    responses((status = 200, description = "订阅 Webhook 配置", body = ApiResponse<SubscriptionConfigVo>))
)]
async fn get_subscription_config(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<SubscriptionConfigVo>, AppError> {
    let row: Option<(String, Option<String>, bool, Option<String>)> = sqlx::query_as(
        "SELECT url, secret, enabled, payload_template
         FROM sys.sys_audit_alert_subscription_config
         WHERE tenant_id = $1 AND user_id = $2",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (url, secret, enabled, payload_template) = row.unwrap_or_default();
    tx.commit().await?;
    Ok(ApiResponse::new(SubscriptionConfigVo {
        url,
        enabled,
        has_secret: secret.map(|s| !s.is_empty()).unwrap_or(false),
        payload_template,
    }))
}

/// 更新当前用户的订阅 Webhook。
#[utoipa::path(
    put,
    path = "/api/system/audit-alert-subscription-config",
    tag = "system",
    request_body = SubscriptionConfigRequest,
    responses((status = 200, description = "保存后的配置", body = ApiResponse<SubscriptionConfigVo>))
)]
async fn update_subscription_config(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<SubscriptionConfigRequest>,
) -> Result<ApiResponse<SubscriptionConfigVo>, AppError> {
    if req.enabled || !req.url.trim().is_empty() {
        // 带域名解析的校验：只查字面量 IP 会放过「解析到内网的公网域名」。
        crate::ssrf::validate_webhook_url_async(&req.url)
            .await
            .map_err(AppError::Validation)?;
    }
    let (has_secret, payload_template): (bool, Option<String>) = sqlx::query_as(
        r#"
        INSERT INTO sys.sys_audit_alert_subscription_config
            (tenant_id, user_id, url, secret, enabled, payload_template, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, now())
        ON CONFLICT (tenant_id, user_id) DO UPDATE
        SET url = EXCLUDED.url,
            secret = COALESCE(EXCLUDED.secret, sys.sys_audit_alert_subscription_config.secret),
            enabled = EXCLUDED.enabled,
            payload_template = COALESCE(EXCLUDED.payload_template, sys.sys_audit_alert_subscription_config.payload_template),
            updated_at = now()
        RETURNING (secret IS NOT NULL AND secret <> ''), payload_template
        "#,
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(&req.url)
    .bind(&req.secret)
    .bind(req.enabled)
    .bind(&req.payload_template)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(ApiResponse::new(SubscriptionConfigVo {
        url: req.url,
        enabled: req.enabled,
        has_secret,
        payload_template,
    }))
}
