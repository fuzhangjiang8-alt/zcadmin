-- =============================================================================
-- 0064_tenant_bill_overdue_tier.sql —— 账单逾期分级
-- =============================================================================

ALTER TABLE sys.sys_tenant_bill
    ADD COLUMN IF NOT EXISTS overdue_tier int NOT NULL DEFAULT 0;
