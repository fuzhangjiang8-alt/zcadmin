-- =============================================================================
-- 0048_password_expiry.sql —— 密码过期
-- =============================================================================

ALTER TABLE sys.sys_user
    ADD COLUMN IF NOT EXISTS password_changed_at timestamptz NOT NULL DEFAULT now();

ALTER TABLE sys.sys_password_policy
    ADD COLUMN IF NOT EXISTS password_expires_days int NOT NULL DEFAULT 0;
