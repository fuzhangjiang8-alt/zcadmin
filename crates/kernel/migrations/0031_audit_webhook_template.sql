-- =============================================================================
-- 0031_audit_webhook_template.sql —— Webhook 自定义 payload 模板
-- =============================================================================

ALTER TABLE sys.sys_audit_webhook ADD COLUMN IF NOT EXISTS payload_template text;
