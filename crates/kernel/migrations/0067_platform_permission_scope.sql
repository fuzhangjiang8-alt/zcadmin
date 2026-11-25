-- =============================================================================
-- 0067_platform_permission_scope.sql —— 平台权限与租户权限分域
--
-- 背景：
--   1) 新建租户时曾把租户管理员写成 is_super = true，而 is_super 在登录时
--      展开为通配 `*`，导致租户管理员可以调用 /api/system/tenants 管理所有租户。
--   2) 权限点表没有「归属层级」，租户管理员在角色授权界面能看到并勾选
--      system:tenant:*，从而给自己提权。
--
-- 本迁移：
--   1) 给 sys_permission 增加 scope（platform / tenant）；
--   2) 把 system:tenant:* 标记为 platform；
--   3) 清理非默认租户角色上的平台级授权；
--   4) 为存量租户补齐 tenant_admin 角色（租户级全部权限），
--      把其 is_super 管理员降级并绑定到该角色。
--
-- 回滚：
--   - 角色/绑定属于数据修正，回滚需人工恢复 is_super（不建议）；
--   - 结构回滚：ALTER TABLE sys.sys_permission DROP COLUMN scope;
--
-- 兼容性：scope 默认 'tenant'，既有插件权限点（crm:*、system:*）自动落为租户级。
-- =============================================================================

ALTER TABLE sys.sys_permission
    ADD COLUMN IF NOT EXISTS scope varchar(16) NOT NULL DEFAULT 'tenant';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'ck_sys_permission_scope'
    ) THEN
        ALTER TABLE sys.sys_permission
            ADD CONSTRAINT ck_sys_permission_scope
            CHECK (scope IN ('platform', 'tenant'));
    END IF;
END $$;

COMMENT ON COLUMN sys.sys_permission.scope IS
    'platform=仅平台超管可见/可授；tenant=租户管理员可授';

-- 平台级权限：租户管理（含账单、清理）
UPDATE sys.sys_permission
SET scope = 'platform'
WHERE code LIKE 'system:tenant:%';

-- 非默认租户的角色不应持有平台级权限（历史误授）
DELETE FROM sys.sys_role_permission rp
USING sys.sys_role r, sys.sys_permission p
WHERE rp.role_id = r.id
  AND rp.permission_id = p.id
  AND p.scope = 'platform'
  AND r.tenant_id <> (
      SELECT id FROM sys.sys_tenant WHERE code = 'default' AND deleted_at IS NULL LIMIT 1
  );

-- 存量租户：补齐 tenant_admin 角色，并降级其超管
DO $$
DECLARE
    t RECORD;
    v_role uuid;
    v_user uuid;
BEGIN
    FOR t IN
        SELECT id FROM sys.sys_tenant
        WHERE deleted_at IS NULL AND code <> 'default'
    LOOP
        -- sys_role / sys_user 都是 FORCE RLS，必须逐租户设置上下文
        PERFORM set_config('app.tenant_id', t.id::text, true);

        SELECT id INTO v_role
        FROM sys.sys_role
        WHERE tenant_id = t.id AND code = 'tenant_admin' AND deleted_at IS NULL
        LIMIT 1;

        IF v_role IS NULL THEN
            INSERT INTO sys.sys_role
                (id, tenant_id, code, name, description, data_scope, is_system)
            VALUES
                (gen_random_uuid(), t.id, 'tenant_admin', '租户管理员',
                 '租户内全部权限（不含平台级）', 'all', true)
            RETURNING id INTO v_role;
        END IF;

        INSERT INTO sys.sys_role_permission (role_id, permission_id)
        SELECT v_role, p.id
        FROM sys.sys_permission p
        WHERE p.scope = 'tenant' AND p.deprecated = false
        ON CONFLICT DO NOTHING;

        FOR v_user IN
            SELECT id FROM sys.sys_user
            WHERE tenant_id = t.id AND is_super AND deleted_at IS NULL
        LOOP
            INSERT INTO sys.sys_user_role (user_id, role_id)
            VALUES (v_user, v_role)
            ON CONFLICT DO NOTHING;

            UPDATE sys.sys_user SET is_super = false WHERE id = v_user;
        END LOOP;
    END LOOP;
END $$;
