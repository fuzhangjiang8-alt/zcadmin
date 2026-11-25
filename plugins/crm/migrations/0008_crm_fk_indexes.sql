SET search_path TO crm;

-- =============================================================================
-- 0008_crm_fk_indexes.sql —— CRM 外键查询索引
--
-- PostgreSQL 不会自动为外键建索引。客户详情、合同回款等按外键过滤的查询，
-- 没有索引时会退化为「扫描本租户全部行再过滤」。
--
-- 全部使用 (tenant_id, fk) 复合索引：RLS 强制带 tenant_id，单列索引意义不大。
-- 部分索引（WHERE deleted_at IS NULL）与软删语义一致，体积更小。
-- =============================================================================

CREATE INDEX IF NOT EXISTS idx_contact_customer
    ON contact (tenant_id, customer_id)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_opportunity_customer
    ON opportunity (tenant_id, customer_id)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_contract_customer
    ON contract (tenant_id, customer_id)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_payment_contract
    ON payment (tenant_id, contract_id)
    WHERE deleted_at IS NULL;
