-- =============================================================================
-- 0057_tenant_delete_permission.sql —— 租户删除权限
-- =============================================================================

INSERT INTO sys.sys_permission (id, plugin, code, name)
VALUES
    ('00000000-0000-7000-8000-000000000404', 'system', 'system:tenant:delete', '删除租户')
ON CONFLICT (code) DO NOTHING;
