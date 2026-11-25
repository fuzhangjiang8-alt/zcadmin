-- =============================================================================
-- 0044_api_token_scopes.sql —— 令牌权限范围
-- =============================================================================

ALTER TABLE sys.sys_api_token
    ADD COLUMN IF NOT EXISTS scopes text[] NOT NULL DEFAULT '{}';
