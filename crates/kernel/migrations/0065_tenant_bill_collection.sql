-- =============================================================================
-- 0065_tenant_bill_collection.sql —— 账单催收记录
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_tenant_bill_collection (
    id                     uuid        PRIMARY KEY,
    bill_id                uuid        NOT NULL REFERENCES sys.sys_tenant_bill (id) ON DELETE CASCADE,
    tenant_id              uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    note                   text        NOT NULL,
    promised_amount_cents  bigint,
    promised_date          date,
    created_by             uuid,
    created_at             timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tenant_bill_collection_bill
    ON sys.sys_tenant_bill_collection (bill_id, created_at DESC);
