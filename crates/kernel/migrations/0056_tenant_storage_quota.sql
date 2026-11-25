-- =============================================================================
-- 0056_tenant_storage_quota.sql —— 租户存储配额
-- =============================================================================

ALTER TABLE sys.sys_tenant
    ADD COLUMN IF NOT EXISTS max_storage_bytes bigint NOT NULL DEFAULT 0;

COMMENT ON COLUMN sys.sys_tenant.max_storage_bytes IS '存储上限（字节），0 表示不限制';
