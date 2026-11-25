-- =============================================================================
-- 联系人（由 cargo xtask gen 生成）
--
-- 注意：本文件生成后即可自由修改；已发布后只能追加新迁移。
-- =============================================================================

CREATE TABLE IF NOT EXISTS contact (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL,
    customer_id uuid NOT NULL ,
    name varchar(64) NOT NULL ,
    phone varchar(32) ,
    email varchar(128) ,
    is_primary boolean NOT NULL ,
    owner_id uuid NOT NULL ,
    dept_id uuid ,
    version     bigint      NOT NULL DEFAULT 0,
    created_at  timestamptz NOT NULL DEFAULT now(),
    created_by  uuid,
    updated_at  timestamptz,
    updated_by  uuid,
    deleted_at  timestamptz
);

COMMENT ON TABLE contact IS '联系人';

CREATE INDEX IF NOT EXISTS idx_contact_list
    ON contact (tenant_id, created_at DESC, id)
    WHERE deleted_at IS NULL;
