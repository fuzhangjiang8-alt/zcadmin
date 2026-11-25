-- =============================================================================
-- 0004_dict.sql —— 数据字典
--
-- 目的：把「枚举值」从代码里搬进数据库，运营可维护，前端按 code 取。
--
-- 设计说明：
--   - 两级结构：字典类型（dict_type）→ 字典项（dict_item）；
--   - type_code 用字符串而不是外键 id：前端调用只关心 `customer_level` 这种语义 code；
--   - 同一租户内 code 唯一；软删后允许复用（部分唯一索引）。
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys_dict_type (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL REFERENCES sys_tenant (id),
    code        varchar(64) NOT NULL,
    name        varchar(64) NOT NULL,
    remark      text,
    status      varchar(16) NOT NULL DEFAULT 'active',
    created_at  timestamptz NOT NULL DEFAULT now(),
    created_by  uuid,
    updated_at  timestamptz,
    updated_by  uuid,
    deleted_at  timestamptz,
    CONSTRAINT ck_sys_dict_type_status CHECK (status IN ('active', 'disabled'))
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_sys_dict_type_code
    ON sys_dict_type (tenant_id, code) WHERE deleted_at IS NULL;

COMMENT ON TABLE sys_dict_type IS '字典类型：一组枚举值的容器，例如 customer_level';

CREATE TABLE IF NOT EXISTS sys_dict_item (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL REFERENCES sys_tenant (id),
    type_code   varchar(64) NOT NULL,
    label       varchar(64) NOT NULL,
    value       varchar(64) NOT NULL,
    sort        int         NOT NULL DEFAULT 0,
    status      varchar(16) NOT NULL DEFAULT 'active',
    created_at  timestamptz NOT NULL DEFAULT now(),
    deleted_at  timestamptz,
    CONSTRAINT ck_sys_dict_item_status CHECK (status IN ('active', 'disabled'))
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_sys_dict_item_value
    ON sys_dict_item (tenant_id, type_code, value) WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_sys_dict_item_type
    ON sys_dict_item (tenant_id, type_code, sort);

COMMENT ON TABLE sys_dict_item IS '字典项：label 给用户看，value 存库与传输';

-- -----------------------------------------------------------------------------
-- 权限点
-- -----------------------------------------------------------------------------
INSERT INTO sys_permission (id, plugin, code, name) VALUES
    ('00000000-0000-7000-8000-000000000108', 'system', 'system:dict:list', '字典查询'),
    ('00000000-0000-7000-8000-000000000109', 'system', 'system:dict:add',  '字典新增'),
    ('00000000-0000-7000-8000-00000000010a', 'system', 'system:dict:edit', '字典编辑')
ON CONFLICT (code) DO NOTHING;

-- -----------------------------------------------------------------------------
-- 示例字典：公告类型（用于验证链路，可删除）
-- -----------------------------------------------------------------------------
INSERT INTO sys_dict_type (id, tenant_id, code, name, remark)
SELECT gen_random_uuid(), t.id, 'notice_type', '公告类型', '系统内置示例字典'
FROM sys_tenant t
WHERE NOT EXISTS (
    SELECT 1 FROM sys_dict_type d WHERE d.tenant_id = t.id AND d.code = 'notice_type'
);

INSERT INTO sys_dict_item (id, tenant_id, type_code, label, value, sort)
SELECT gen_random_uuid(), t.id, 'notice_type', v.label, v.value, v.sort
FROM sys_tenant t
CROSS JOIN (VALUES ('通知', 'notice', 1), ('公告', 'announcement', 2), ('警告', 'warning', 3))
    AS v(label, value, sort)
WHERE NOT EXISTS (
    SELECT 1 FROM sys_dict_item i
    WHERE i.tenant_id = t.id AND i.type_code = 'notice_type' AND i.value = v.value
);
