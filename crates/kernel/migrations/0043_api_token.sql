-- =============================================================================
-- 0043_api_token.sql —— 个人访问令牌（PAT）
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_api_token (
    id           uuid        PRIMARY KEY,
    tenant_id    uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    user_id      uuid        NOT NULL REFERENCES sys.sys_user (id) ON DELETE CASCADE,
    name         text        NOT NULL,
    token_hash   text        NOT NULL UNIQUE,
    expires_at   timestamptz,
    last_used_at timestamptz,
    revoked_at   timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_api_token_user
    ON sys.sys_api_token (tenant_id, user_id, created_at DESC);

ALTER TABLE sys.sys_api_token ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_api_token;
CREATE POLICY tenant_isolation ON sys.sys_api_token
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_api_token FORCE ROW LEVEL SECURITY;
