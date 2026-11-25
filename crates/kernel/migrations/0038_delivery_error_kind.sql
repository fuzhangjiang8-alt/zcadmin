-- =============================================================================
-- 0038_delivery_error_kind.sql —— 投递错误分类
-- =============================================================================

ALTER TABLE sys.sys_audit_alert_delivery
    ADD COLUMN IF NOT EXISTS last_error_kind text;
