//! 审计配置包模型：版本、元数据、配置包结构、迁移与差异计算。
//!
//! 边界：导出/导入/预览在 `audit_admin` 的配置 IO 部分；快照在独立模块。
//! 关键约束：配置包不包含 secret；版本迁移必须保持向后兼容。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::audit_rules::AuditAlertRuleInput;
use crate::error::AppError;

/// 当前配置包版本。
pub const AUDIT_CONFIG_VERSION: i32 = 2;

fn default_config_version() -> i32 {
    AUDIT_CONFIG_VERSION
}

/// 配置包元数据。
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigMetadata {
    /// 导出时间。
    pub exported_at: DateTime<Utc>,
    /// 应用版本。
    pub app_version: String,
}

/// 审计配置导出包（不含 secret）。
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditConfigBundle {
    /// 配置包版本。
    #[serde(default = "default_config_version")]
    pub version: i32,
    /// 导出元数据（v2 新增）。
    #[serde(default)]
    pub metadata: Option<ConfigMetadata>,
    /// 告警规则。
    pub alert_rules: Vec<AuditAlertRuleInput>,
    /// 当前用户订阅模式。
    pub subscription_patterns: Vec<String>,
    /// 订阅 Webhook 地址。
    pub subscription_webhook_url: String,
    /// 订阅 Webhook 是否启用。
    pub subscription_webhook_enabled: bool,
    /// 租户 Webhook 地址。
    pub webhook_url: String,
    /// 租户 Webhook 是否启用。
    pub webhook_enabled: bool,
    /// 租户 Webhook 模板。
    pub webhook_payload_template: Option<String>,
}

/// 选择性导入参数。
#[derive(Debug, Deserialize)]
pub struct ConfigSectionsQuery {
    /// 逗号分隔的范围：rules,subscriptions,webhook。
    pub sections: Option<String>,
    /// 操作模式：import（默认）/ rollback。
    pub mode: Option<String>,
}

/// 配置导入差异。
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditConfigDiffVo {
    /// 新增规则。
    pub rules_added: Vec<String>,
    /// 删除规则。
    pub rules_removed: Vec<String>,
    /// 变更规则。
    pub rules_changed: Vec<String>,
    /// 新增订阅。
    pub subscriptions_added: Vec<String>,
    /// 删除订阅。
    pub subscriptions_removed: Vec<String>,
    /// 租户 Webhook 是否变化。
    pub webhook_changed: bool,
    /// 订阅 Webhook 是否变化。
    pub subscription_webhook_changed: bool,
}

pub(crate) fn migrate_config_bundle(
    mut bundle: AuditConfigBundle,
) -> Result<AuditConfigBundle, AppError> {
    match bundle.version {
        1 => {
            // v1 -> v2：仅新增 metadata，字段本身兼容
            bundle.version = 2;
            Ok(bundle)
        }
        2 => Ok(bundle),
        v => Err(AppError::Validation(format!("不支持的配置包版本: {v}"))),
    }
}

pub(crate) fn compute_config_diff(
    old: &AuditConfigBundle,
    new: &AuditConfigBundle,
) -> AuditConfigDiffVo {
    let old_rules: std::collections::HashMap<&str, &AuditAlertRuleInput> = old
        .alert_rules
        .iter()
        .map(|r| (r.action_pattern.as_str(), r))
        .collect();
    let new_rules: std::collections::HashMap<&str, &AuditAlertRuleInput> = new
        .alert_rules
        .iter()
        .map(|r| (r.action_pattern.as_str(), r))
        .collect();
    let mut rules_added: Vec<String> = new_rules
        .keys()
        .filter(|p| !old_rules.contains_key(*p))
        .map(|p| p.to_string())
        .collect();
    let mut rules_removed: Vec<String> = old_rules
        .keys()
        .filter(|p| !new_rules.contains_key(*p))
        .map(|p| p.to_string())
        .collect();
    let mut rules_changed: Vec<String> = new_rules
        .iter()
        .filter(|(p, rule)| {
            old_rules.get(*p).is_some_and(|old| {
                old.window_minutes != rule.window_minutes
                    || old.threshold != rule.threshold
                    || old.silence_minutes != rule.silence_minutes
                    || old.enabled != rule.enabled
            })
        })
        .map(|(p, _)| p.to_string())
        .collect();
    rules_added.sort();
    rules_removed.sort();
    rules_changed.sort();

    let old_subs: std::collections::HashSet<&str> = old
        .subscription_patterns
        .iter()
        .map(|p| p.as_str())
        .collect();
    let new_subs: std::collections::HashSet<&str> = new
        .subscription_patterns
        .iter()
        .map(|p| p.as_str())
        .collect();
    let mut subscriptions_added: Vec<String> = new_subs
        .difference(&old_subs)
        .map(|p| p.to_string())
        .collect();
    let mut subscriptions_removed: Vec<String> = old_subs
        .difference(&new_subs)
        .map(|p| p.to_string())
        .collect();
    subscriptions_added.sort();
    subscriptions_removed.sort();

    AuditConfigDiffVo {
        rules_added,
        rules_removed,
        rules_changed,
        subscriptions_added,
        subscriptions_removed,
        webhook_changed: old.webhook_url != new.webhook_url
            || old.webhook_enabled != new.webhook_enabled
            || old.webhook_payload_template != new.webhook_payload_template,
        subscription_webhook_changed: old.subscription_webhook_url != new.subscription_webhook_url
            || old.subscription_webhook_enabled != new.subscription_webhook_enabled,
    }
}
