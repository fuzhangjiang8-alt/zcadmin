-- =============================================================================
-- 0012_rls_system.sql —— RLS 策略：组织 / 角色 / 字典 / 审计
--
-- 同样**不 FORCE**：应用仍是表 owner，策略暂不生效。
-- 等 system.rs 全部 handler 回归通过后，再逐表 FORCE。
--
-- 未包含 sys_user / sys_menu：
--   - sys_user 的登录查询发生在「还不知道租户」时，需要先做租户解析（子域/租户码）；
--   - sys_menu 有 api 与 seed 两条写入路径，需要先统一到 TenantTx。
-- =============================================================================

-- 部门
ALTER TABLE sys_dept ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys_dept;
CREATE POLICY tenant_isolation ON sys_dept
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

-- 角色
ALTER TABLE sys_role ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys_role;
CREATE POLICY tenant_isolation ON sys_role
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

-- 字典类型
ALTER TABLE sys_dict_type ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys_dict_type;
CREATE POLICY tenant_isolation ON sys_dict_type
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

-- 字典项（dict_item 自带 tenant_id）
ALTER TABLE sys_dict_item ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys_dict_item;
CREATE POLICY tenant_isolation ON sys_dict_item
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

-- 审计日志：只允许插入与查询，不允许修改/删除
ALTER TABLE sys_audit_log ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys_audit_log;
CREATE POLICY tenant_isolation ON sys_audit_log
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);
