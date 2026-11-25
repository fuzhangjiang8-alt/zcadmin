-- =============================================================================
-- 0047_password_policy.sql —— 租户密码策略
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_password_policy (
    tenant_id      uuid        PRIMARY KEY REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    min_length     int         NOT NULL DEFAULT 10,
    require_upper  boolean     NOT NULL DEFAULT true,
    require_lower  boolean     NOT NULL DEFAULT true,
    require_digit  boolean     NOT NULL DEFAULT true,
    history_count  int         NOT NULL DEFAULT 5,
    updated_at     timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE sys.sys_password_policy ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_password_policy;
CREATE POLICY tenant_isolation ON sys.sys_password_policy
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_password_policy FORCE ROW LEVEL SECURITY;
