-- =============================================================================
-- 0010_payment_covering.sql —— 回款覆盖索引
--
-- 1) 合同回款明细：按 paid_at 排序取最近 500 条 + 汇总金额
--    INCLUDE (amount_cents) 让 SUM 走 Index Only Scan，避免 5 万次回表。
-- 2) 经营看板：按租户汇总回款金额
--    (tenant_id) INCLUDE (amount_cents) 同样支持 Index Only Scan。
-- =============================================================================

DROP INDEX IF EXISTS idx_payment_contract;
CREATE INDEX idx_payment_contract
    ON payment (tenant_id, contract_id, paid_at DESC)
    INCLUDE (amount_cents)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_payment_tenant_amount
    ON payment (tenant_id)
    INCLUDE (amount_cents)
    WHERE deleted_at IS NULL;
