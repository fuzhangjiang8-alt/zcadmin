-- =============================================================================
-- 0009_payment_index_paid_at.sql —— 回款明细排序优化
--
-- 合同回款明细按 paid_at DESC 排序取最近 500 条。
-- 原索引 (tenant_id, contract_id) 仍需排序 5 万行；把 paid_at 并入索引可消除 Sort。
--
-- 注意：0008 已应用，按迁移规范只能新增，不能改历史迁移。
-- =============================================================================

DROP INDEX IF EXISTS idx_payment_contract;

CREATE INDEX idx_payment_contract
    ON payment (tenant_id, contract_id, paid_at DESC)
    WHERE deleted_at IS NULL;
