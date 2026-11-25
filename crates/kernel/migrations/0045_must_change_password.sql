-- =============================================================================
-- 0045_must_change_password.sql —— 首次登录强制改密
-- =============================================================================

ALTER TABLE sys.sys_user
    ADD COLUMN IF NOT EXISTS must_change_password boolean NOT NULL DEFAULT false;

ALTER TABLE sys.sys_session
    ADD COLUMN IF NOT EXISTS must_change_password boolean NOT NULL DEFAULT false;
