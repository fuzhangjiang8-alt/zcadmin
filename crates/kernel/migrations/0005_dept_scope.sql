-- =============================================================================
-- 0005_dept_scope.sql —— 部门与数据权限
--
-- 目的：
--   1. 建立组织架构（部门树），用户归属部门
--   2. 给角色增加数据范围（data_scope），实现「只能看本部门/本人」的行级权限
--
-- 数据范围取值：
--   all            全部数据（超管/高管）
--   dept_and_child 本部门及下级部门
--   dept           仅本部门
--   self           仅本人
--
-- 兼容性：新增表 + 新增可空列，旧代码不受影响。
-- 回滚：DROP TABLE sys_dept; ALTER TABLE sys_user DROP COLUMN dept_id;
--       ALTER TABLE sys_role DROP COLUMN data_scope;
-- 风险：sys_user 加列在 PG 11+ 不重写表，锁时间极短。
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys_dept (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL REFERENCES sys_tenant (id),
    parent_id   uuid        REFERENCES sys_dept (id) ON DELETE RESTRICT,
    name        varchar(64) NOT NULL,
    sort        int         NOT NULL DEFAULT 0,
    leader_id   uuid,
    created_at  timestamptz NOT NULL DEFAULT now(),
    deleted_at  timestamptz
);

CREATE INDEX IF NOT EXISTS idx_sys_dept_tenant_parent
    ON sys_dept (tenant_id, parent_id);

COMMENT ON TABLE sys_dept IS '部门：树形组织架构，数据权限的划分依据';

-- 用户归属部门（可空：历史数据/外部账号可能不归属任何部门）
ALTER TABLE sys_user ADD COLUMN IF NOT EXISTS dept_id uuid REFERENCES sys_dept (id);

-- 角色的数据范围
ALTER TABLE sys_role ADD COLUMN IF NOT EXISTS data_scope varchar(16) NOT NULL DEFAULT 'self';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'ck_sys_role_data_scope'
    ) THEN
        ALTER TABLE sys_role ADD CONSTRAINT ck_sys_role_data_scope
            CHECK (data_scope IN ('all', 'dept_and_child', 'dept', 'self'));
    END IF;
END $$;

COMMENT ON COLUMN sys_role.data_scope IS '数据范围：all/dept_and_child/dept/self';

-- 内置超管角色拥有全部数据范围
UPDATE sys_role SET data_scope = 'all' WHERE code = 'super_admin';

-- -----------------------------------------------------------------------------
-- 为每个租户初始化一个根部门，并把已有用户挂上去
-- -----------------------------------------------------------------------------
INSERT INTO sys_dept (id, tenant_id, parent_id, name, sort)
SELECT gen_random_uuid(), t.id, NULL, '总公司', 0
FROM sys_tenant t
WHERE NOT EXISTS (
    SELECT 1 FROM sys_dept d WHERE d.tenant_id = t.id AND d.parent_id IS NULL
);

UPDATE sys_user u
SET dept_id = d.id
FROM sys_dept d
WHERE u.dept_id IS NULL
  AND d.tenant_id = u.tenant_id
  AND d.parent_id IS NULL;

-- -----------------------------------------------------------------------------
-- 新增权限点：查看敏感字段
-- -----------------------------------------------------------------------------
INSERT INTO sys_permission (id, plugin, code, name) VALUES
    ('00000000-0000-7000-8000-00000000010b', 'system', 'system:user:view_sensitive', '查看用户敏感字段')
ON CONFLICT (code) DO NOTHING;
