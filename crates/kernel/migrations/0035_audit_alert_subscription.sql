-- =============================================================================
-- 0035_audit_alert_subscription.sql —— 告警订阅（站内）
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_audit_alert_subscription (
    id             uuid        PRIMARY KEY,
    tenant_id      uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    user_id        uuid        NOT NULL REFERENCES sys.sys_user (id) ON DELETE CASCADE,
    action_pattern text        NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, user_id, action_pattern)
);

ALTER TABLE sys.sys_audit_alert_subscription ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_audit_alert_subscription;
CREATE POLICY tenant_isolation ON sys.sys_audit_alert_subscription
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_audit_alert_subscription FORCE ROW LEVEL SECURITY;
