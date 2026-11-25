-- =============================================================================
-- 0033_audit_alert_retry.sql —— Webhook 投递重试退避
-- =============================================================================

ALTER TABLE sys.sys_audit_alert
    ADD COLUMN IF NOT EXISTS notify_attempts int NOT NULL DEFAULT 0;

ALTER TABLE sys.sys_audit_alert
    ADD COLUMN IF NOT EXISTS next_notify_at timestamptz;

CREATE INDEX IF NOT EXISTS idx_sys_audit_alert_pending
    ON sys.sys_audit_alert (tenant_id, next_notify_at)
    WHERE notified_at IS NULL;
