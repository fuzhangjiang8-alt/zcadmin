-- =============================================================================
-- demo 插件初始迁移
--
-- 执行上下文：`SET search_path TO demo` 之后，因此表名不带 schema 前缀。
-- 约定：插件只能写自己的 schema；跨插件访问必须通过对方提供的只读视图。
-- =============================================================================

CREATE TABLE IF NOT EXISTS demo_item (
    id          uuid        PRIMARY KEY,
    name        varchar(64) NOT NULL,
    note        text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    created_by  uuid,
    deleted_at  timestamptz
);

-- 列表页默认按创建时间倒序；带上 id 保证同一时间的排序稳定。
CREATE INDEX IF NOT EXISTS idx_demo_item_created
    ON demo_item (created_at DESC, id)
    WHERE deleted_at IS NULL;

COMMENT ON TABLE demo_item IS '示例实体：用于验证插件契约与 CRUD 生成器';
COMMENT ON COLUMN demo_item.name IS '名称，1~64 字符，应用层校验';
