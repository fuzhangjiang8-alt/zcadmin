-- =============================================================================
-- 0016_login_lockout.sql —— 登录失败锁定
--
-- 连续失败 N 次后锁定账号一段时间，防止在线暴力破解。
-- 计数放在用户行上：多实例部署也能共享状态（不需要 Redis）。
-- =============================================================================

ALTER TABLE sys_user ADD COLUMN IF NOT EXISTS failed_attempts int NOT NULL DEFAULT 0;
ALTER TABLE sys_user ADD COLUMN IF NOT EXISTS locked_until timestamptz;

COMMENT ON COLUMN sys_user.failed_attempts IS '连续登录失败次数，成功后清零';
COMMENT ON COLUMN sys_user.locked_until IS '锁定截止时间，NULL 表示未锁定';
