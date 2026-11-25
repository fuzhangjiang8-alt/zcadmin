-- =============================================================================
-- 0054_tenant_permissions.sql —— 租户管理权限点
-- =============================================================================

INSERT INTO sys.sys_permission (id, plugin, code, name)
VALUES
    ('00000000-0000-7000-8000-000000000401', 'system', 'system:tenant:list', '租户列表'),
    ('00000000-0000-7000-8000-000000000402', 'system', 'system:tenant:add', '创建租户'),
    ('00000000-0000-7000-8000-000000000403', 'system', 'system:tenant:edit', '租户状态管理')
ON CONFLICT (code) DO NOTHING;
