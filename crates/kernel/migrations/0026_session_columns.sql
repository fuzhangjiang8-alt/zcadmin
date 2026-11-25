-- =============================================================================
-- 0026_session_columns.sql
--
-- 0001 早已预留 sys_session，但字段是「M0 预留版」：
-- 缺 dept_id / data_scope / username / permissions。
-- 0025 的 CREATE TABLE IF NOT EXISTS 因此不会改结构，这里补列。
-- =============================================================================

ALTER TABLE sys.sys_session ADD COLUMN IF NOT EXISTS dept_id uuid;
ALTER TABLE sys.sys_session ADD COLUMN IF NOT EXISTS data_scope text NOT NULL DEFAULT 'self';
ALTER TABLE sys.sys_session ADD COLUMN IF NOT EXISTS username text NOT NULL DEFAULT '';
ALTER TABLE sys.sys_session ADD COLUMN IF NOT EXISTS permissions jsonb NOT NULL DEFAULT '[]'::jsonb;

CREATE INDEX IF NOT EXISTS idx_sys_session_tenant
    ON sys.sys_session (tenant_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_sys_session_expires
    ON sys.sys_session (expires_at);
