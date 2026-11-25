-- =============================================================================
-- 0073_permission_scope_default_platform.sql —— 权限点 scope 默认值改为 platform
--
-- 背景：
--   0067 给 sys_permission 增加了 scope（platform / tenant），但列默认值是 'tenant'。
--   于是「新增一个平台级权限点、忘了写 scope」会静默落成租户级，
--   任何租户管理员都能在角色授权页面给自己勾上 —— 默认放行，与最小权限相反。
--
-- 本迁移：
--   把默认值改成 'platform'（默认拒绝）。插件权限点由
--   system::sync_plugin_permissions 显式写入 scope = 'tenant'，不受影响。
--
-- 影响范围：
--   只影响**将来**不带 scope 的 INSERT；已有行的 scope 保持不变。
--
-- 回滚：
--   ALTER TABLE sys.sys_permission ALTER COLUMN scope SET DEFAULT 'tenant';
-- =============================================================================

ALTER TABLE sys.sys_permission
    ALTER COLUMN scope SET DEFAULT 'platform';

COMMENT ON COLUMN sys.sys_permission.scope IS
    'platform=仅平台超管可见/可授（默认值）；tenant=租户管理员可授。新增权限点必须先判层级。';
