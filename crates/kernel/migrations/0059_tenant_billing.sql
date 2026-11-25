-- =============================================================================
-- 0059_tenant_billing.sql —— 租户计费单价
-- =============================================================================

ALTER TABLE sys.sys_tenant
    ADD COLUMN IF NOT EXISTS price_per_user_month_cents int NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS price_per_gb_month_cents int NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS currency text NOT NULL DEFAULT 'CNY';

COMMENT ON COLUMN sys.sys_tenant.price_per_user_month_cents IS '每用户每月单价（分）';
COMMENT ON COLUMN sys.sys_tenant.price_per_gb_month_cents IS '每 GB 存储每月单价（分）';
