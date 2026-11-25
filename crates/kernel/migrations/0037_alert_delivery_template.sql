-- =============================================================================
-- 0037_alert_delivery_template.sql —— 订阅 Webhook payload 模板
-- =============================================================================

ALTER TABLE sys.sys_audit_alert_subscription_config
    ADD COLUMN IF NOT EXISTS payload_template text;

ALTER TABLE sys.sys_audit_alert_delivery
    ADD COLUMN IF NOT EXISTS payload_template text;
