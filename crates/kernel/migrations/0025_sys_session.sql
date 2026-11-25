-- =============================================================================
-- 0025_sys_session.sql —— 会话持久化（多实例共享）
--
-- 为什么不做 RLS：登录时先按令牌查会话才知道 tenant_id，
-- 此时还没有租户上下文；会话表只按 token_hash / tenant_id 精确访问。
-- 令牌不落库，只存 SHA-256 十六进制摘要。
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_session (
    id           uuid        PRIMARY KEY,
    token_hash   text        NOT NULL UNIQUE,
    tenant_id    uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    user_id      uuid        NOT NULL REFERENCES sys.sys_user (id) ON DELETE CASCADE,
    dept_id      uuid,
    data_scope   text        NOT NULL,
    username     text        NOT NULL,
    permissions  jsonb       NOT NULL DEFAULT '[]'::jsonb,
    created_at   timestamptz NOT NULL DEFAULT now(),
    expires_at   timestamptz NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_sys_session_tenant
    ON sys.sys_session (tenant_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_sys_session_expires
    ON sys.sys_session (expires_at);
