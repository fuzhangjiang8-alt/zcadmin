-- =============================================================================
-- 0063_tenant_bill_overdue.sql —— 账单逾期提醒时间
-- =============================================================================

ALTER TABLE sys.sys_tenant_bill
    ADD COLUMN IF NOT EXISTS overdue_notified_at timestamptz;
