-- =============================================================================
-- 0015_contract_opportunity.sql —— 商机 → 合同转化
--
-- 合同可选关联来源商机；一个商机可以转化多份合同（分期/续签）。
-- ON DELETE SET NULL：商机删除不应连带删除已签合同。
-- =============================================================================

ALTER TABLE contract
    ADD COLUMN IF NOT EXISTS opportunity_id uuid
        REFERENCES opportunity (id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_contract_opportunity
    ON contract (tenant_id, opportunity_id)
    WHERE deleted_at IS NULL;
