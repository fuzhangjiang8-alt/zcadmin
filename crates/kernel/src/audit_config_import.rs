//! 审计配置包导入。
//!
//! 边界：导出在 `audit_config_export`；预览在 `audit_config_preview`。
//! 关键约束：导入在单事务内完成；支持按 sections 选择性导入。

use axum::extract::{Query, State};
use axum::routing::put;
use axum::Json;
use axum::Router;

use crate::audit_config_bundle::{
    compute_config_diff, migrate_config_bundle, AuditConfigBundle, ConfigSectionsQuery,
    AUDIT_CONFIG_VERSION,
};
use crate::audit_rules::AuditAlertRuleInput;
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;

/// 配置导入路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/audit-config/import", put(import_audit_config))
}

/// 导入审计配置（覆盖规则与订阅；secret 保持不变）。
#[utoipa::path(
    put,
    path = "/api/system/audit-config/import",
    tag = "system",
    request_body = AuditConfigBundle,
    responses((status = 200, description = "导入后的配置", body = ApiResponse<AuditConfigBundle>))
)]
async fn import_audit_config(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<ConfigSectionsQuery>,
    Json(bundle): Json<AuditConfigBundle>,
) -> Result<ApiResponse<AuditConfigBundle>, AppError> {
    require_perm(&user, crate::perms::AUDIT_ALERT)?;
    let bundle = migrate_config_bundle(bundle)?;
    let sections: Vec<String> = q
        .sections
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let all_sections = sections.is_empty();
    let do_rules = all_sections || sections.iter().any(|s| s == "rules");
    let do_subs = all_sections || sections.iter().any(|s| s == "subscriptions");
    let do_webhook = all_sections || sections.iter().any(|s| s == "webhook");
    let is_rollback = q.mode.as_deref() == Some("rollback");
    let config_action = if is_rollback {
        "system.config.rollback"
    } else {
        "system.config.import"
    };
    let audit_action = if is_rollback {
        "system.audit.config.rollback"
    } else {
        "system.audit.config.import"
    };
    if do_rules {
        if !bundle.alert_rules.iter().any(|r| r.action_pattern == "*") {
            return Err(AppError::Validation("规则必须包含 *".into()));
        }
        let mut seen = std::collections::HashSet::new();
        for rule in &bundle.alert_rules {
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
                return Err(AppError::Validation("规则参数超出范围".into()));
            }
        }
    }
    if do_subs {
        for pattern in &bundle.subscription_patterns {
            if pattern.trim().is_empty() || pattern.len() > 128 {
                return Err(AppError::Validation("订阅模式非法".into()));
            }
        }
        // 导入是「整包落库」的旁路：不做这一步，配置包就能把 Webhook 指向内网，
        // 后台投递任务会按该地址发请求（盲 SSRF）。
        if bundle.subscription_webhook_enabled || !bundle.subscription_webhook_url.trim().is_empty()
        {
            crate::ssrf::validate_webhook_url_async(&bundle.subscription_webhook_url)
                .await
                .map_err(AppError::Validation)?;
        }
    }
    if do_webhook && (bundle.webhook_enabled || !bundle.webhook_url.trim().is_empty()) {
        crate::ssrf::validate_webhook_url_async(&bundle.webhook_url)
            .await
            .map_err(AppError::Validation)?;
    }

    // 落库前快照当前配置，供回滚使用
    let prev_rules: Vec<AuditAlertRuleInput> = sqlx::query_as(
        "SELECT action_pattern, window_minutes, threshold, silence_minutes, enabled
         FROM sys.sys_audit_alert_rule WHERE tenant_id = $1",
    )
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;
    let prev_patterns: Vec<String> = sqlx::query_scalar(
        "SELECT action_pattern FROM sys.sys_audit_alert_subscription
         WHERE tenant_id = $1 AND user_id = $2",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_all(&mut *tx)
    .await?;
    let prev_sub_cfg: Option<(String, bool)> = sqlx::query_as(
        "SELECT url, enabled FROM sys.sys_audit_alert_subscription_config
         WHERE tenant_id = $1 AND user_id = $2",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let prev_hook: Option<(String, bool, Option<String>)> = sqlx::query_as(
        "SELECT url, enabled, payload_template FROM sys.sys_audit_webhook WHERE tenant_id = $1",
    )
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (prev_sub_url, prev_sub_enabled) = prev_sub_cfg.unwrap_or_default();
    let (prev_hook_url, prev_hook_enabled, prev_hook_template) = prev_hook.unwrap_or_default();
    let prev_bundle = AuditConfigBundle {
        version: AUDIT_CONFIG_VERSION,
        metadata: None,
        alert_rules: prev_rules,
        subscription_patterns: prev_patterns,
        subscription_webhook_url: prev_sub_url,
        subscription_webhook_enabled: prev_sub_enabled,
        webhook_url: prev_hook_url,
        webhook_enabled: prev_hook_enabled,
        webhook_payload_template: prev_hook_template,
    };
    let snapshot_sections = if all_sections {
        "all".to_string()
    } else {
        sections.join(",")
    };
    let diff_summary = serde_json::json!({
        "rules": if do_rules { bundle.alert_rules.len() } else { 0 },
        "subscriptions": if do_subs { bundle.subscription_patterns.len() } else { 0 },
    });
    let diff_detail = compute_config_diff(&prev_bundle, &bundle);
    sqlx::query(
        "INSERT INTO sys.sys_audit_config_snapshot
            (id, tenant_id, created_by, bundle, sections, diff_summary, diff_detail)
         VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6)",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(sqlx::types::Json(&prev_bundle))
    .bind(&snapshot_sections)
    .bind(sqlx::types::Json(&diff_summary))
    .bind(sqlx::types::Json(&diff_detail))
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM sys.sys_audit_config_snapshot
         WHERE tenant_id = $1 AND id NOT IN (
             SELECT id FROM sys.sys_audit_config_snapshot
             WHERE tenant_id = $1 ORDER BY created_at DESC LIMIT 20
         )",
    )
    .bind(user.tenant_id)
    .execute(&mut *tx)
    .await?;

    if do_rules {
        sqlx::query("DELETE FROM sys.sys_audit_alert_rule WHERE tenant_id = $1")
            .bind(user.tenant_id)
            .execute(&mut *tx)
            .await?;
    }
    if do_rules {
        for rule in &bundle.alert_rules {
            sqlx::query(
                "INSERT INTO sys.sys_audit_alert_rule
                (id, tenant_id, action_pattern, window_minutes, threshold, silence_minutes, enabled)
             VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6)",
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
    }

    if do_subs {
        sqlx::query(
            "DELETE FROM sys.sys_audit_alert_subscription WHERE tenant_id = $1 AND user_id = $2",
        )
        .bind(user.tenant_id)
        .bind(user.user_id)
        .execute(&mut *tx)
        .await?;
        for pattern in &bundle.subscription_patterns {
            sqlx::query(
                "INSERT INTO sys.sys_audit_alert_subscription (id, tenant_id, user_id, action_pattern)
                 VALUES (gen_random_uuid(), $1, $2, $3) ON CONFLICT DO NOTHING",
            )
            .bind(user.tenant_id)
            .bind(user.user_id)
            .bind(pattern.trim())
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            "INSERT INTO sys.sys_audit_alert_subscription_config
                (tenant_id, user_id, url, enabled, updated_at)
             VALUES ($1, $2, $3, $4, now())
             ON CONFLICT (tenant_id, user_id) DO UPDATE
             SET url = EXCLUDED.url, enabled = EXCLUDED.enabled, updated_at = now()",
        )
        .bind(user.tenant_id)
        .bind(user.user_id)
        .bind(&bundle.subscription_webhook_url)
        .bind(bundle.subscription_webhook_enabled)
        .execute(&mut *tx)
        .await?;
    }

    if do_webhook {
        sqlx::query(
            "INSERT INTO sys.sys_audit_webhook
                (tenant_id, url, enabled, payload_template, updated_at)
             VALUES ($1, $2, $3, $4, now())
             ON CONFLICT (tenant_id) DO UPDATE
             SET url = EXCLUDED.url,
                 enabled = EXCLUDED.enabled,
                 payload_template = EXCLUDED.payload_template,
                 updated_at = now()",
        )
        .bind(user.tenant_id)
        .bind(&bundle.webhook_url)
        .bind(bundle.webhook_enabled)
        .bind(&bundle.webhook_payload_template)
        .execute(&mut *tx)
        .await?;
    }

    // 配置变更告警：复用告警/Webhook 通知通道
    sqlx::query(
        r#"
        INSERT INTO sys.sys_audit_alert
            (id, tenant_id, action, window_start, window_end, count, threshold)
        VALUES (
            gen_random_uuid(), $1, $2,
            date_trunc('minute', now()), now(), 1, 1
        )
        ON CONFLICT (tenant_id, action, window_start)
        DO UPDATE SET count = sys.sys_audit_alert.count + 1, window_end = now()
        "#,
    )
    .bind(user.tenant_id)
    .bind(config_action)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        audit_action,
        "audit_config",
        None,
        Some(serde_json::json!({
            "rules": bundle.alert_rules.len(),
            "mode": if is_rollback { "rollback" } else { "import" }
        })),
    )
    .await;

    Ok(ApiResponse::new(bundle))
}
