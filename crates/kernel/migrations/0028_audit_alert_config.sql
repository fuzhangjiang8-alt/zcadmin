-- =============================================================================
-- 0028_audit_alert_config.sql —— 审计告警阈值配置
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_audit_alert_config (
    tenant_id      uuid        PRIMARY KEY REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    window_minutes int         NOT NULL DEFAULT 5,
    threshold      int         NOT NULL DEFAULT 50,
    updated_at     timestamptz NOT NULL DEFAULT now(),
    updated_by     uuid
);

INSERT INTO sys.sys_audit_alert_config (tenant_id)
SELECT id FROM sys.sys_tenant
ON CONFLICT (tenant_id) DO NOTHING;

ALTER TABLE sys.sys_audit_alert_config ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_audit_alert_config;
CREATE POLICY tenant_isolation ON sys.sys_audit_alert_config
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_audit_alert_config FORCE ROW LEVEL SECURITY;
