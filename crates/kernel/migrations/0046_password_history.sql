-- =============================================================================
-- 0046_password_history.sql —— 密码历史（防复用）
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_password_history (
    id            uuid        PRIMARY KEY,
    tenant_id     uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    user_id       uuid        NOT NULL REFERENCES sys.sys_user (id) ON DELETE CASCADE,
    password_hash text        NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_password_history_user
    ON sys.sys_password_history (tenant_id, user_id, created_at DESC);

ALTER TABLE sys.sys_password_history ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_password_history;
CREATE POLICY tenant_isolation ON sys.sys_password_history
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_password_history FORCE ROW LEVEL SECURITY;
