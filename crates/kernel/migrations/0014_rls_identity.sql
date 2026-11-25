-- =============================================================================
-- 0014_rls_identity.sql —— RLS 策略：sys_user / sys_menu
--
-- 登录路径已支持「租户编码」解析（见 auth::resolve_tenant），
-- 因此 sys_user 可以按租户隔离。仍然**不 FORCE**：
--   - ensure_seed / ensure_default_menus 在启动时按租户循环写入，
--     它们还没有 set_config，FORCE 后会被策略拦截；
--   - 下一步把 seed 也改成「每个租户一个事务」，再统一 FORCE。
-- =============================================================================

ALTER TABLE sys_user ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys_user;
CREATE POLICY tenant_isolation ON sys_user
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys_menu ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys_menu;
CREATE POLICY tenant_isolation ON sys_menu
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);
