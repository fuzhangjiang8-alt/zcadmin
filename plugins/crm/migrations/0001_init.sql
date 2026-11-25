-- =============================================================================
-- CRM 插件初始迁移：客户主数据
--
-- 约定：
--   - 表在 `crm` schema 下（装配器执行前会 SET search_path）
--   - 必备列：id / tenant_id / version / 审计四件套 / deleted_at
--   - 软删 + 部分唯一索引：删除后允许复用编码
-- =============================================================================

CREATE TABLE IF NOT EXISTS customer (
    id            uuid        PRIMARY KEY,
    tenant_id     uuid        NOT NULL,
    code          varchar(32) NOT NULL,
    name          varchar(128) NOT NULL,
    level         varchar(32) NOT NULL DEFAULT 'normal',
    status        varchar(16) NOT NULL DEFAULT 'potential',
    -- 归属：数据权限按 owner_id / dept_id 过滤
    owner_id      uuid        NOT NULL,
    dept_id       uuid,
    phone         varchar(32),
    email         varchar(128),
    remark        text,
    version       bigint      NOT NULL DEFAULT 0,
    created_at    timestamptz NOT NULL DEFAULT now(),
    created_by    uuid,
    updated_at    timestamptz,
    updated_by    uuid,
    deleted_at    timestamptz,
    CONSTRAINT ck_customer_status
        CHECK (status IN ('potential', 'following', 'signed', 'lost'))
);

COMMENT ON TABLE customer IS '客户主数据：签约主体，租户内 code 唯一';
COMMENT ON COLUMN customer.level IS '客户等级，取值来自字典 customer_level';
COMMENT ON COLUMN customer.status IS '生命周期：潜在/跟进中/已签约/已流失';
COMMENT ON COLUMN customer.owner_id IS '负责人，数据权限的 self 依据';
COMMENT ON COLUMN customer.dept_id IS '归属部门，数据权限的 dept 依据';

-- 列表默认按创建时间倒序；tenant_id 放最左，保证 RLS 与多租户查询能走索引
CREATE INDEX IF NOT EXISTS idx_customer_list
    ON customer (tenant_id, created_at DESC, id)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_customer_owner
    ON customer (tenant_id, owner_id)
    WHERE deleted_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS uq_customer_code
    ON customer (tenant_id, code)
    WHERE deleted_at IS NULL;
