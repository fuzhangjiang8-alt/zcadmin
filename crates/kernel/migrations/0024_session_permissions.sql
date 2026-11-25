-- =============================================================================
-- 0024_session_permissions.sql —— 在线会话管理
-- =============================================================================

INSERT INTO sys.sys_permission (id, plugin, code, name) VALUES
    ('00000000-0000-7000-8000-000000000b01', 'system', 'system:session:list',   '会话查询'),
    ('00000000-0000-7000-8000-000000000b02', 'system', 'system:session:revoke', '强制下线')
ON CONFLICT (code) DO NOTHING;

INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
SELECT gen_random_uuid(), t.id, p.id, '在线会话', '/system/sessions', 'desktop', 'system:session:list', 17
FROM sys.sys_tenant t
JOIN sys.sys_menu p ON p.tenant_id = t.id AND p.path = '/system'
WHERE NOT EXISTS (
    SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = t.id AND m.path = '/system/sessions'
);
