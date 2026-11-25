-- =============================================================================
-- 0034_audit_alert_dead_letter.sql —— Webhook 最大重试与死信
-- =============================================================================

ALTER TABLE sys.sys_audit_alert
    ADD COLUMN IF NOT EXISTS notify_failed_at timestamptz;

CREATE INDEX IF NOT EXISTS idx_sys_audit_alert_pending
    ON sys.sys_audit_alert (tenant_id, next_notify_at)
    WHERE notified_at IS NULL AND notify_failed_at IS NULL;
