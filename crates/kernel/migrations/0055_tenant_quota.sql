-- =============================================================================
-- 0055_tenant_quota.sql —— 租户用户数配额
-- =============================================================================

ALTER TABLE sys.sys_tenant
    ADD COLUMN IF NOT EXISTS max_users int NOT NULL DEFAULT 0;

COMMENT ON COLUMN sys.sys_tenant.max_users IS '用户数上限，0 表示不限制';
