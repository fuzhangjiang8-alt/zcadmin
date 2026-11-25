-- =============================================================================
-- 0066_tenant_bill_collection_reminder.sql —— 催收承诺提醒
-- =============================================================================

ALTER TABLE sys.sys_tenant_bill_collection
    ADD COLUMN IF NOT EXISTS reminded_at timestamptz;
