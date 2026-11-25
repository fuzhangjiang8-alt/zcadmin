-- =============================================================================
-- 0003_menu.sql —— 菜单与基础权限点
--
-- 目的：
--   1. 建立菜单表（前端侧边栏与路由的唯一来源）
--   2. 初始化系统管理相关的权限点
--
-- 设计说明：
--   - 菜单是「数据」不是「代码」：新增页面只需要插一条记录，不需要改前端路由表；
--   - 菜单通过 perm 关联权限点，登录后按用户权限过滤，无权限的菜单不下发；
--   - path 在同一租户内唯一，避免前端路由冲突。
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys_menu (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL REFERENCES sys_tenant (id),
    parent_id   uuid        REFERENCES sys_menu (id) ON DELETE CASCADE,
    title       varchar(64) NOT NULL,
    path        varchar(128) NOT NULL,
    icon        varchar(64),
    -- 需要的权限点；为空表示登录即可见
    perm        varchar(128),
    sort        int         NOT NULL DEFAULT 0,
    visible     boolean     NOT NULL DEFAULT true,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_sys_menu_path
    ON sys_menu (tenant_id, path);

COMMENT ON TABLE sys_menu IS '菜单：前端侧边栏与动态路由的唯一来源';

-- -----------------------------------------------------------------------------
-- 基础权限点（幂等插入，重复启动不会报错）
-- -----------------------------------------------------------------------------
INSERT INTO sys_permission (id, plugin, code, name) VALUES
    ('00000000-0000-7000-8000-000000000101', 'system', 'system:user:list', '用户查询'),
    ('00000000-0000-7000-8000-000000000102', 'system', 'system:user:add',  '用户新增'),
    ('00000000-0000-7000-8000-000000000103', 'system', 'system:user:edit', '用户编辑'),
    ('00000000-0000-7000-8000-000000000104', 'system', 'system:role:list', '角色查询'),
    ('00000000-0000-7000-8000-000000000105', 'system', 'system:role:add',  '角色新增'),
    ('00000000-0000-7000-8000-000000000106', 'system', 'system:role:edit', '角色授权'),
    ('00000000-0000-7000-8000-000000000107', 'system', 'system:menu:list', '菜单查询')
ON CONFLICT (code) DO NOTHING;

-- -----------------------------------------------------------------------------
-- 默认菜单（为每个已存在的租户插入一次）
-- -----------------------------------------------------------------------------
INSERT INTO sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
SELECT gen_random_uuid(), t.id, NULL, '工作台', '/', 'home', NULL, 0
FROM sys_tenant t
WHERE NOT EXISTS (SELECT 1 FROM sys_menu m WHERE m.tenant_id = t.id AND m.path = '/');

INSERT INTO sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
SELECT gen_random_uuid(), t.id, NULL, '系统管理', '/system', 'setting', NULL, 10
FROM sys_tenant t
WHERE NOT EXISTS (SELECT 1 FROM sys_menu m WHERE m.tenant_id = t.id AND m.path = '/system');

INSERT INTO sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
SELECT gen_random_uuid(), t.id, p.id, '用户管理', '/system/users', 'team', 'system:user:list', 11
FROM sys_tenant t
JOIN sys_menu p ON p.tenant_id = t.id AND p.path = '/system'
WHERE NOT EXISTS (SELECT 1 FROM sys_menu m WHERE m.tenant_id = t.id AND m.path = '/system/users');

INSERT INTO sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
SELECT gen_random_uuid(), t.id, p.id, '角色管理', '/system/roles', 'safety', 'system:role:list', 12
FROM sys_tenant t
JOIN sys_menu p ON p.tenant_id = t.id AND p.path = '/system'
WHERE NOT EXISTS (SELECT 1 FROM sys_menu m WHERE m.tenant_id = t.id AND m.path = '/system/roles');
