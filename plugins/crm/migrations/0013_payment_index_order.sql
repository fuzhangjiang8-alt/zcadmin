-- =============================================================================
-- 0013_payment_index_order.sql
--
-- 0010 的索引是 (tenant_id, contract_id, paid_at DESC)，
-- 但查询是 ORDER BY paid_at DESC NULLS LAST, created_at DESC：
-- 显式 NULLS LAST 与索引默认的 DESC NULLS FIRST 不匹配，规划器无法用索引排序，
-- 退化为 Parallel Seq Scan + top-N heapsort（5 万行约 50ms）。
--
-- 重建为与查询完全一致的排序，并包含 created_at 作为并列排序键。
-- =============================================================================

DROP INDEX IF EXISTS idx_payment_contract;

CREATE INDEX idx_payment_contract
    ON payment (tenant_id, contract_id, paid_at DESC NULLS LAST, created_at DESC)
    INCLUDE (amount_cents)
    WHERE deleted_at IS NULL;
