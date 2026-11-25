-- =============================================================================
-- 0039_audit_config_snapshot.sql —— 审计配置导入快照
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_audit_config_snapshot (
    id         uuid        PRIMARY KEY,
    tenant_id  uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    created_by uuid        REFERENCES sys.sys_user (id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    bundle     jsonb       NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_config_snapshot_tenant
    ON sys.sys_audit_config_snapshot (tenant_id, created_at DESC);

ALTER TABLE sys.sys_audit_config_snapshot ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_audit_config_snapshot;
CREATE POLICY tenant_isolation ON sys.sys_audit_config_snapshot
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_audit_config_snapshot FORCE ROW LEVEL SECURITY;
