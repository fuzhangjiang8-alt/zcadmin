//! 审计配置包导出。
//!
//! 边界：导入在 `audit_config_import`；预览在 `audit_config_preview`。
//! 关键约束：导出不包含任何 secret；版本号取自 `AUDIT_CONFIG_VERSION`。

use axum::extract::State;
use axum::routing::get;
use axum::Router;
use chrono::Utc;

use crate::audit_config_bundle::{AuditConfigBundle, ConfigMetadata, AUDIT_CONFIG_VERSION};
use crate::audit_rules::AuditAlertRuleInput;
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;

/// 配置导出路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/audit-config/export", get(export_audit_config))
}

/// 导出审计配置。
#[utoipa::path(
    get,
    path = "/api/system/audit-config/export",
    tag = "system",
    responses((status = 200, description = "配置包", body = ApiResponse<AuditConfigBundle>))
)]
async fn export_audit_config(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<AuditConfigBundle>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let alert_rules: Vec<AuditAlertRuleInput> = sqlx::query_as(
        "SELECT action_pattern, window_minutes, threshold, silence_minutes, enabled
         FROM sys.sys_audit_alert_rule WHERE tenant_id = $1
         ORDER BY length(action_pattern) DESC, action_pattern",
    )
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;
    let subscription_patterns: Vec<String> = sqlx::query_scalar(
        "SELECT action_pattern FROM sys.sys_audit_alert_subscription
         WHERE tenant_id = $1 AND user_id = $2 ORDER BY action_pattern",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_all(&mut *tx)
    .await?;
    let sub_cfg: Option<(String, bool)> = sqlx::query_as(
        "SELECT url, enabled FROM sys.sys_audit_alert_subscription_config
         WHERE tenant_id = $1 AND user_id = $2",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let hook: Option<(String, bool, Option<String>)> = sqlx::query_as(
        "SELECT url, enabled, payload_template FROM sys.sys_audit_webhook WHERE tenant_id = $1",
    )
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    let (subscription_webhook_url, subscription_webhook_enabled) = sub_cfg.unwrap_or_default();
    let (webhook_url, webhook_enabled, webhook_payload_template) = hook.unwrap_or_default();
    Ok(ApiResponse::new(AuditConfigBundle {
        version: AUDIT_CONFIG_VERSION,
        metadata: Some(ConfigMetadata {
            exported_at: Utc::now(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        }),
        alert_rules,
        subscription_patterns,
        subscription_webhook_url,
        subscription_webhook_enabled,
        webhook_url,
        webhook_enabled,
        webhook_payload_template,
    }))
}
