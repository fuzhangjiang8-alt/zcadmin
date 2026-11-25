-- =============================================================================
-- 0029_audit_webhook.sql —— 审计告警 Webhook
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_audit_webhook (
    tenant_id  uuid        PRIMARY KEY REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    url        text        NOT NULL DEFAULT '',
    secret     text,
    enabled    boolean     NOT NULL DEFAULT false,
    updated_at timestamptz NOT NULL DEFAULT now(),
    updated_by uuid
);

ALTER TABLE sys.sys_audit_alert ADD COLUMN IF NOT EXISTS notified_at timestamptz;

ALTER TABLE sys.sys_audit_webhook ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_audit_webhook;
CREATE POLICY tenant_isolation ON sys.sys_audit_webhook
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_audit_webhook FORCE ROW LEVEL SECURITY;
