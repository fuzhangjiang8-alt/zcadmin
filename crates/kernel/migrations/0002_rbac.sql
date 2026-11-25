-- =============================================================================
-- 0002_rbac.sql —— 角色与权限
--
-- 目的：在 M0 的 sys_user 基础上补齐 RBAC 三元组
--       （用户 → 角色 → 权限点），供登录时计算权限集。
--
-- 兼容性：全部为新增表，不影响已有数据与代码（Expand 阶段）。
-- 回滚：DROP TABLE sys_role_permission, sys_user_role, sys_permission, sys_role;
-- 风险：仅新建空表，无锁表风险。
-- =============================================================================

-- 角色
CREATE TABLE IF NOT EXISTS sys_role (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL REFERENCES sys_tenant (id),
    code        varchar(64) NOT NULL,
    name        varchar(64) NOT NULL,
    description text,
    -- 内置角色（如 super_admin）不允许删除，避免把自己锁在系统外
    is_system   boolean     NOT NULL DEFAULT false,
    version     bigint      NOT NULL DEFAULT 0,
    created_at  timestamptz NOT NULL DEFAULT now(),
    created_by  uuid,
    updated_at  timestamptz,
    updated_by  uuid,
    deleted_at  timestamptz
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_sys_role_code
    ON sys_role (tenant_id, code) WHERE deleted_at IS NULL;

COMMENT ON TABLE sys_role IS '角色：权限的集合，用户通过角色获得权限';

-- 权限点
--
-- code 形如 `crm:customer:add`，由插件在启动时声明并 upsert。
-- 注意：权限点**不做物理删除**，下线时置 deprecated = true，
--       否则角色配置会丢失（用户下次登录权限莫名变少）。
CREATE TABLE IF NOT EXISTS sys_permission (
    id          uuid        PRIMARY KEY,
    plugin      varchar(64) NOT NULL,
    code        varchar(128) NOT NULL,
    name        varchar(128) NOT NULL,
    deprecated  boolean     NOT NULL DEFAULT false,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_sys_permission_code ON sys_permission (code);

COMMENT ON TABLE sys_permission IS '权限点：最小授权单元，格式 {plugin}:{resource}:{action}';

-- 用户-角色
CREATE TABLE IF NOT EXISTS sys_user_role (
    user_id     uuid        NOT NULL REFERENCES sys_user (id) ON DELETE CASCADE,
    role_id     uuid        NOT NULL REFERENCES sys_role (id) ON DELETE CASCADE,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, role_id)
);

COMMENT ON TABLE sys_user_role IS '用户与角色的多对多关系';

-- 角色-权限
CREATE TABLE IF NOT EXISTS sys_role_permission (
    role_id         uuid        NOT NULL REFERENCES sys_role (id) ON DELETE CASCADE,
    permission_id   uuid        NOT NULL REFERENCES sys_permission (id) ON DELETE CASCADE,
    created_at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (role_id, permission_id)
);

COMMENT ON TABLE sys_role_permission IS '角色与权限点的多对多关系';

-- 内置超级管理员角色（权限判定走 is_super 短路，不依赖此角色，仅用于展示）
INSERT INTO sys_role (id, tenant_id, code, name, description, is_system)
SELECT
    '00000000-0000-7000-8000-000000000001'::uuid,
    t.id,
    'super_admin',
    '超级管理员',
    '系统内置角色，拥有全部权限',
    true
FROM sys_tenant t
WHERE NOT EXISTS (
    SELECT 1 FROM sys_role r
    WHERE r.tenant_id = t.id AND r.code = 'super_admin'
);
