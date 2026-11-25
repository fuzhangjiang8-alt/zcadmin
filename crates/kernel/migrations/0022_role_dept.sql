-- =============================================================================
-- 0022_role_dept.sql —— 角色自定义数据范围（指定部门）
--
-- 角色 data_scope = 'custom' 时，可见部门由本表决定。
-- 多角色：取并集（范围优先级 all > dept_and_child > custom > dept > self）。
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_role_dept (
    tenant_id uuid NOT NULL,
    role_id   uuid NOT NULL REFERENCES sys.sys_role (id) ON DELETE CASCADE,
    dept_id   uuid NOT NULL REFERENCES sys.sys_dept (id) ON DELETE CASCADE,
    PRIMARY KEY (role_id, dept_id)
);

CREATE INDEX IF NOT EXISTS idx_sys_role_dept_tenant
    ON sys.sys_role_dept (tenant_id, role_id);

ALTER TABLE sys.sys_role_dept ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_role_dept;
CREATE POLICY tenant_isolation ON sys.sys_role_dept
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_role_dept FORCE ROW LEVEL SECURITY;
