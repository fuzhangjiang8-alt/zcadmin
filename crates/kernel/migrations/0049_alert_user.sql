-- =============================================================================
-- 0049_alert_user.sql —— 告警关联用户
-- =============================================================================

ALTER TABLE sys.sys_audit_alert
    ADD COLUMN IF NOT EXISTS user_id uuid REFERENCES sys.sys_user (id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_audit_alert_user
    ON sys.sys_audit_alert (tenant_id, user_id, created_at DESC)
    WHERE user_id IS NOT NULL;
