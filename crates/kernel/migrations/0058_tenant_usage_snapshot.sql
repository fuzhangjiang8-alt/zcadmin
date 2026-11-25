-- =============================================================================
-- 0058_tenant_usage_snapshot.sql —— 租户用量日快照
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_tenant_usage_snapshot (
    id             uuid        PRIMARY KEY,
    tenant_id      uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    snapshot_date  date        NOT NULL,
    user_count     bigint      NOT NULL DEFAULT 0,
    file_count     bigint      NOT NULL DEFAULT 0,
    storage_used   bigint      NOT NULL DEFAULT 0,
    customer_count bigint      NOT NULL DEFAULT 0,
    contract_count bigint      NOT NULL DEFAULT 0,
    created_at     timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT uq_tenant_usage_day UNIQUE (tenant_id, snapshot_date)
);

CREATE INDEX IF NOT EXISTS idx_tenant_usage_tenant_day
    ON sys.sys_tenant_usage_snapshot (tenant_id, snapshot_date DESC);
