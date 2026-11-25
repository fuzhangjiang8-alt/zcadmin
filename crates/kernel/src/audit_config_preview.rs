//! 审计配置包导入预览（不落库）。
//!
//! 边界：实际导入在 `audit_admin` 的导入 handler；本模块只计算差异预览。
//! 关键约束：预览不写库、不改配置；只返回将发生的变化。

use axum::extract::{Query, State};
use axum::routing::post;
use axum::Json;
use axum::Router;

use crate::audit_config_bundle::{
    migrate_config_bundle, AuditConfigBundle, AuditConfigDiffVo, ConfigSectionsQuery,
};
use crate::audit_rules::AuditAlertRuleInput;
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;

/// 配置预览路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/audit-config/preview", post(preview_audit_config))
}

/// 预览审计配置导入差异（不落库）。
#[utoipa::path(
    post,
    path = "/api/system/audit-config/preview",
    tag = "system",
    request_body = AuditConfigBundle,
    responses((status = 200, description = "差异", body = ApiResponse<AuditConfigDiffVo>))
)]
async fn preview_audit_config(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<ConfigSectionsQuery>,
    Json(bundle): Json<AuditConfigBundle>,
) -> Result<ApiResponse<AuditConfigDiffVo>, AppError> {
    require_perm(&user, crate::perms::AUDIT_ALERT)?;
    let bundle = migrate_config_bundle(bundle)?;
    let preview_sections: Vec<String> = q
        .sections
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let preview_all = preview_sections.is_empty();
    let preview_do_rules = preview_all || preview_sections.iter().any(|s| s == "rules");
    let preview_do_subs = preview_all || preview_sections.iter().any(|s| s == "subscriptions");
    let preview_do_webhook = preview_all || preview_sections.iter().any(|s| s == "webhook");

    let current_rules: Vec<AuditAlertRuleInput> = sqlx::query_as(
        "SELECT action_pattern, window_minutes, threshold, silence_minutes, enabled
         FROM sys.sys_audit_alert_rule WHERE tenant_id = $1",
    )
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;
    let current_patterns: Vec<String> = sqlx::query_scalar(
        "SELECT action_pattern FROM sys.sys_audit_alert_subscription
         WHERE tenant_id = $1 AND user_id = $2",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_all(&mut *tx)
    .await?;
    let current_sub_cfg: Option<(String, bool)> = sqlx::query_as(
        "SELECT url, enabled FROM sys.sys_audit_alert_subscription_config
         WHERE tenant_id = $1 AND user_id = $2",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let current_hook: Option<(String, bool, Option<String>)> = sqlx::query_as(
        "SELECT url, enabled, payload_template FROM sys.sys_audit_webhook WHERE tenant_id = $1",
    )
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;

    let old_rules: std::collections::HashMap<String, AuditAlertRuleInput> = current_rules
        .into_iter()
        .map(|r| (r.action_pattern.clone(), r))
        .collect();
    let new_rules: std::collections::HashMap<String, AuditAlertRuleInput> = bundle
        .alert_rules
        .iter()
        .map(|r| (r.action_pattern.clone(), (*r).clone()))
        .collect();
    let mut rules_added = Vec::new();
    let mut rules_changed = Vec::new();
    for (pattern, rule) in &new_rules {
        match old_rules.get(pattern) {
            None => rules_added.push(pattern.clone()),
            Some(old)
                if old.window_minutes != rule.window_minutes
                    || old.threshold != rule.threshold
                    || old.silence_minutes != rule.silence_minutes
                    || old.enabled != rule.enabled =>
            {
                rules_changed.push(pattern.clone());
            }
            _ => {}
        }
    }
    let rules_removed: Vec<String> = old_rules
        .keys()
        .filter(|p| !new_rules.contains_key(*p))
        .cloned()
        .collect();

    let old_subs: std::collections::HashSet<String> = current_patterns.into_iter().collect();
    let new_subs: std::collections::HashSet<String> = bundle
        .subscription_patterns
        .iter()
        .map(|p| p.trim().to_string())
        .collect();
    let mut subscriptions_added: Vec<String> = new_subs.difference(&old_subs).cloned().collect();
    let mut subscriptions_removed: Vec<String> = old_subs.difference(&new_subs).cloned().collect();
    subscriptions_added.sort();
    subscriptions_removed.sort();

    let (sub_url, sub_enabled) = current_sub_cfg.unwrap_or_default();
    let (hook_url, hook_enabled, hook_template) = current_hook.unwrap_or_default();
    let subscription_webhook_changed = sub_url != bundle.subscription_webhook_url
        || sub_enabled != bundle.subscription_webhook_enabled;
    let webhook_changed = hook_url != bundle.webhook_url
        || hook_enabled != bundle.webhook_enabled
        || hook_template != bundle.webhook_payload_template;

    rules_added.sort();
    rules_changed.sort();
    Ok(ApiResponse::new(AuditConfigDiffVo {
        rules_added: if preview_do_rules {
            rules_added
        } else {
            Vec::new()
        },
        rules_removed: if preview_do_rules {
            rules_removed
        } else {
            Vec::new()
        },
        rules_changed: if preview_do_rules {
            rules_changed
        } else {
            Vec::new()
        },
        subscriptions_added: if preview_do_subs {
            subscriptions_added
        } else {
            Vec::new()
        },
        subscriptions_removed: if preview_do_subs {
            subscriptions_removed
        } else {
            Vec::new()
        },
        webhook_changed: preview_do_webhook && webhook_changed,
        subscription_webhook_changed: preview_do_subs && subscription_webhook_changed,
    }))
}
