-- =============================================================================
-- 0068_rls_association_tables.sql —— 关联表的行级隔离
--
-- 背景：
--   sys_user_role / sys_role_permission / sys_user_post 是三张没有 tenant_id 的
--   关联表。此前它们既没有 RLS，也没有租户校验，任何持有 user:edit 的租户管理员
--   只要知道别租户的 user_id / role_id，就能跨租户改绑定关系。
--
-- 为什么不加 tenant_id 列：
--   1) 三张表都能通过父表（sys_user / sys_role）唯一确定租户；
--   2) 加列需要回填 + 改所有写入点，回归面更大；
--   3) 用「父表可见性」做策略，天然跟随 sys_user / sys_role 的 FORCE RLS，
--      不会出现两处租户字段不一致的漂移。
--
-- 性能：
--   策略里的 IN 子查询走父表主键，且 RBAC 表数据量小（百级），可接受。
--
-- 回滚：
--   DROP POLICY tenant_isolation ON ...; ALTER TABLE ... DISABLE ROW LEVEL SECURITY;
-- =============================================================================

-- 用户-角色：可见性 = 用户在当前租户可见
ALTER TABLE sys.sys_user_role ENABLE ROW LEVEL SECURITY;
ALTER TABLE sys.sys_user_role FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_user_role;
CREATE POLICY tenant_isolation ON sys.sys_user_role
    USING (user_id IN (SELECT u.id FROM sys.sys_user u))
    WITH CHECK (user_id IN (SELECT u.id FROM sys.sys_user u));

-- 角色-权限：可见性 = 角色在当前租户可见
ALTER TABLE sys.sys_role_permission ENABLE ROW LEVEL SECURITY;
ALTER TABLE sys.sys_role_permission FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_role_permission;
CREATE POLICY tenant_isolation ON sys.sys_role_permission
    USING (role_id IN (SELECT r.id FROM sys.sys_role r))
    WITH CHECK (role_id IN (SELECT r.id FROM sys.sys_role r));

-- 用户-岗位：可见性 = 用户在当前租户可见
ALTER TABLE sys.sys_user_post ENABLE ROW LEVEL SECURITY;
ALTER TABLE sys.sys_user_post FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_user_post;
CREATE POLICY tenant_isolation ON sys.sys_user_post
    USING (user_id IN (SELECT u.id FROM sys.sys_user u))
    WITH CHECK (user_id IN (SELECT u.id FROM sys.sys_user u));

COMMENT ON POLICY tenant_isolation ON sys.sys_user_role IS
    '租户隔离：绑定关系只看得到当前租户的用户';
COMMENT ON POLICY tenant_isolation ON sys.sys_role_permission IS
    '租户隔离：授权关系只看得到当前租户的角色';
COMMENT ON POLICY tenant_isolation ON sys.sys_user_post IS
    '租户隔离：岗位关系只看得到当前租户的用户';
