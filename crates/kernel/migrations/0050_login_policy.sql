-- =============================================================================
-- 0050_login_policy.sql —— 租户登录风控策略
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_login_policy (
    tenant_id           uuid        PRIMARY KEY REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    max_failed_attempts int         NOT NULL DEFAULT 5,
    lock_seconds        int         NOT NULL DEFAULT 900,
    warning_percent     int         NOT NULL DEFAULT 60,
    updated_at          timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE sys.sys_login_policy ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_login_policy;
CREATE POLICY tenant_isolation ON sys.sys_login_policy
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_login_policy FORCE ROW LEVEL SECURITY;
