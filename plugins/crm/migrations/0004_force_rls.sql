-- =============================================================================
-- 0004_force_rls.sql —— 正式启用 CRM 业务表的行级安全
--
-- 依赖 0003_rls.sql 创建的策略；真机冒烟（客户 CRUD）通过后启用。
-- =============================================================================

ALTER TABLE customer FORCE ROW LEVEL SECURITY;
ALTER TABLE contact  FORCE ROW LEVEL SECURITY;
