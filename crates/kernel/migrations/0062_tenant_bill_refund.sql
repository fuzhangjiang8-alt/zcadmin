-- =============================================================================
-- 0062_tenant_bill_refund.sql —— 账单退款/冲正
-- =============================================================================

ALTER TABLE sys.sys_tenant_bill_payment
    ADD COLUMN IF NOT EXISTS kind text NOT NULL DEFAULT 'payment';

ALTER TABLE sys.sys_tenant_bill_payment
    DROP CONSTRAINT IF EXISTS ck_tenant_bill_payment_kind;

ALTER TABLE sys.sys_tenant_bill_payment
    ADD CONSTRAINT ck_tenant_bill_payment_kind CHECK (kind IN ('payment', 'refund'));
