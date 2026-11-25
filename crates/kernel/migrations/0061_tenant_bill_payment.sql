-- =============================================================================
-- 0061_tenant_bill_payment.sql —— 租户账单收款流水
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_tenant_bill_payment (
    id           uuid        PRIMARY KEY,
    bill_id      uuid        NOT NULL REFERENCES sys.sys_tenant_bill (id) ON DELETE CASCADE,
    tenant_id    uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    amount_cents bigint      NOT NULL CHECK (amount_cents > 0),
    method       text        NOT NULL DEFAULT 'bank',
    reference    text,
    note         text,
    created_by   uuid,
    created_at   timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tenant_bill_payment_bill
    ON sys.sys_tenant_bill_payment (bill_id, created_at);
