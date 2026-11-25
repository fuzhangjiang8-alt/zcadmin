-- =============================================================================
-- 0010_rls_config.sql —— 行级安全（RLS）试点：sys_config
--
-- 策略：租户只能看到 tenant_id = current_setting('app.tenant_id') 的行。
--
-- ⚠️ 重要：这里**没有** FORCE ROW LEVEL SECURITY。
--    - 表 owner（应用连接）仍然可以绕过策略，因此本迁移是「准备阶段」；
--    - 等所有 handler 都改用 TenantTx（请求级事务注入 app.tenant_id）后，
--      再执行 `ALTER TABLE ... FORCE ROW LEVEL SECURITY` 正式生效。
--
-- 回滚：DROP POLICY ...; ALTER TABLE ... DISABLE ROW LEVEL SECURITY;
-- 风险：仅新增策略，不改变现有查询结果（未 FORCE）。
-- =============================================================================

ALTER TABLE sys_config ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS tenant_isolation ON sys_config;

CREATE POLICY tenant_isolation ON sys_config
    USING (
        tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid
    );

COMMENT ON POLICY tenant_isolation ON sys_config IS
    '租户隔离：依赖请求级事务注入的 app.tenant_id；未设置时看不到任何行';
