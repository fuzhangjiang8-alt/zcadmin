-- =============================================================================
-- 0003_rls.sql —— CRM 业务表的行级安全策略
--
-- 插件迁移在 kernel 迁移之后执行，因此可以安全引用 crm schema 的表。
-- 与内核保持一致：**不 FORCE**，等 FORCE 阶段统一启用。
-- =============================================================================

ALTER TABLE customer ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON customer;
CREATE POLICY tenant_isolation ON customer
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE contact ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON contact;
CREATE POLICY tenant_isolation ON contact
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);
