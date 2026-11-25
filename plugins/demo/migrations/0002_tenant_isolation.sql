-- =============================================================================
-- 0002_tenant_isolation.sql —— demo 插件接入多租户
--
-- 背景：demo_item 建表时没有 tenant_id，也没有 RLS。多租户部署下
--       任何租户都能读写所有租户的示例数据；而 demo 是团队复制模板，
--       不修就会把缺陷复制到新插件。
--
-- 处理：
--   1) 补 tenant_id；存量数据归到最早创建的租户（示例数据可丢弃）；
--   2) 启用并 FORCE RLS，策略与 CRM 插件保持一致；
--   3) 增加 (tenant_id, created_at) 索引，匹配列表查询排序。
-- =============================================================================

ALTER TABLE demo_item ADD COLUMN IF NOT EXISTS tenant_id uuid;

UPDATE demo_item
SET tenant_id = (SELECT id FROM sys.sys_tenant ORDER BY created_at, id LIMIT 1)
WHERE tenant_id IS NULL;

-- 没有任何租户时（极端情况）保留 NULL 会阻塞 NOT NULL，直接清掉示例数据。
DELETE FROM demo_item WHERE tenant_id IS NULL;

ALTER TABLE demo_item ALTER COLUMN tenant_id SET NOT NULL;

ALTER TABLE demo_item ENABLE ROW LEVEL SECURITY;
ALTER TABLE demo_item FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON demo_item;
CREATE POLICY tenant_isolation ON demo_item
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

CREATE INDEX IF NOT EXISTS idx_demo_item_tenant_created
    ON demo_item (tenant_id, created_at DESC, id)
    WHERE deleted_at IS NULL;

COMMENT ON COLUMN demo_item.tenant_id IS '所属租户（RLS 隔离键）';
