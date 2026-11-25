-- =============================================================================
-- 0007_notice.sql —— 站内通知
--
-- 设计：
--   - `sys_notice` 存通知正文（租户级，所有用户可见）；
--   - `sys_notice_read` 存「谁在什么时候读过」，用复合主键天然去重；
--   - 已读状态不放在通知表上（否则无法支持多用户）。
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys_notice (
    id           uuid        PRIMARY KEY,
    tenant_id    uuid        NOT NULL REFERENCES sys_tenant (id),
    title        varchar(200) NOT NULL,
    content      text        NOT NULL,
    level        varchar(16) NOT NULL DEFAULT 'info',
    status       varchar(16) NOT NULL DEFAULT 'published',
    publisher_id uuid,
    published_at timestamptz NOT NULL DEFAULT now(),
    created_at   timestamptz NOT NULL DEFAULT now(),
    deleted_at   timestamptz,
    CONSTRAINT ck_sys_notice_level CHECK (level IN ('info', 'success', 'warning', 'error')),
    CONSTRAINT ck_sys_notice_status CHECK (status IN ('draft', 'published'))
);

CREATE INDEX IF NOT EXISTS idx_sys_notice_list
    ON sys_notice (tenant_id, published_at DESC, id)
    WHERE deleted_at IS NULL;

COMMENT ON TABLE sys_notice IS '站内通知：租户内广播，已读状态按用户记录';

CREATE TABLE IF NOT EXISTS sys_notice_read (
    notice_id uuid        NOT NULL REFERENCES sys_notice (id) ON DELETE CASCADE,
    user_id   uuid        NOT NULL,
    read_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (notice_id, user_id)
);

COMMENT ON TABLE sys_notice_read IS '通知已读记录：复合主键保证幂等';

INSERT INTO sys_permission (id, plugin, code, name) VALUES
    ('00000000-0000-7000-8000-00000000010f', 'system', 'system:notice:list', '通知查询'),
    ('00000000-0000-7000-8000-000000000110', 'system', 'system:notice:add',  '通知发布')
ON CONFLICT (code) DO NOTHING;
