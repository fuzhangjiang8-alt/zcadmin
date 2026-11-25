-- =============================================================================
-- 0006_file.sql —— 文件元数据
--
-- 设计：
--   - 二进制内容存对象存储（本地磁盘 / S3 / MinIO），数据库只存元数据；
--   - 存储 key 由服务端生成（`{tenant}/{file_id}`），**不使用原始文件名**，
--     避免路径穿越与同名覆盖；
--   - 软删：元数据保留 30 天，物理文件由清理任务回收。
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys_file (
    id           uuid        PRIMARY KEY,
    tenant_id    uuid        NOT NULL REFERENCES sys_tenant (id),
    name         varchar(255) NOT NULL,
    mime         varchar(128) NOT NULL,
    size_bytes   bigint      NOT NULL,
    storage_key  text        NOT NULL,
    uploader_id  uuid,
    created_at   timestamptz NOT NULL DEFAULT now(),
    deleted_at   timestamptz
);

CREATE INDEX IF NOT EXISTS idx_sys_file_tenant_time
    ON sys_file (tenant_id, created_at DESC)
    WHERE deleted_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS uq_sys_file_storage_key
    ON sys_file (storage_key);

COMMENT ON TABLE sys_file IS '文件元数据：二进制内容存放在对象存储';
COMMENT ON COLUMN sys_file.storage_key IS '对象存储 key，由服务端生成，不含用户输入';

INSERT INTO sys_permission (id, plugin, code, name) VALUES
    ('00000000-0000-7000-8000-00000000010c', 'system', 'system:file:list',   '文件查询'),
    ('00000000-0000-7000-8000-00000000010d', 'system', 'system:file:upload', '文件上传'),
    ('00000000-0000-7000-8000-00000000010e', 'system', 'system:file:delete', '文件删除')
ON CONFLICT (code) DO NOTHING;
