-- =============================================================================
-- 0060_tenant_bill.sql —— 租户月度账单
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_tenant_bill (
    id                     uuid        PRIMARY KEY,
    tenant_id              uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    bill_month             date        NOT NULL,
    currency               text        NOT NULL,
    avg_users              numeric(12,2) NOT NULL DEFAULT 0,
    avg_storage_bytes      bigint      NOT NULL DEFAULT 0,
    user_amount_cents      bigint      NOT NULL DEFAULT 0,
    storage_amount_cents   bigint      NOT NULL DEFAULT 0,
    total_cents            bigint      NOT NULL DEFAULT 0,
    status                 text        NOT NULL DEFAULT 'draft',
    created_at             timestamptz NOT NULL DEFAULT now(),
    updated_at             timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT uq_tenant_bill_month UNIQUE (tenant_id, bill_month),
    CONSTRAINT ck_tenant_bill_status CHECK (status IN ('draft', 'issued', 'paid', 'void'))
);

CREATE INDEX IF NOT EXISTS idx_tenant_bill_month ON sys.sys_tenant_bill (bill_month, tenant_id);
