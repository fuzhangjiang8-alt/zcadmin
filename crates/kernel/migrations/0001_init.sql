-- =============================================================================
-- 内核初始迁移（sys schema）
--
-- 约定：
--   1. 迁移文件一旦发布不可修改，只能新增；
--   2. 所有业务表必备列：id / tenant_id / version / created_at / created_by /
--      updated_at / updated_by / deleted_at；
--   3. 时间一律用 timestamptz（禁止无时区的 timestamp）；
--   4. 主键用 uuid（应用侧生成 UUIDv7），不用自增。
--
-- 注意：本文件在 `SET search_path TO sys` 之后执行，因此表名不带 schema 前缀。
-- =============================================================================

-- 租户表：多租户的根。单租户私有化部署时也会有一条默认记录。
CREATE TABLE IF NOT EXISTS sys_tenant (
    id          uuid        PRIMARY KEY,
    code        varchar(64) NOT NULL,
    name        varchar(128) NOT NULL,
    status      varchar(16) NOT NULL DEFAULT 'active',
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz,
    deleted_at  timestamptz,
    CONSTRAINT uq_sys_tenant_code UNIQUE (code),
    CONSTRAINT ck_sys_tenant_status CHECK (status IN ('active', 'suspended', 'deleted'))
);

COMMENT ON TABLE sys_tenant IS '租户：多租户隔离的根实体';

-- 用户表
CREATE TABLE IF NOT EXISTS sys_user (
    id             uuid        PRIMARY KEY,
    tenant_id      uuid        NOT NULL REFERENCES sys_tenant (id),
    username       varchar(64) NOT NULL,
    display_name   varchar(64) NOT NULL,
    password_hash  text        NOT NULL,
    email          varchar(128),
    phone          varchar(32),
    status         varchar(16) NOT NULL DEFAULT 'active',
    is_super       boolean     NOT NULL DEFAULT false,
    version        bigint      NOT NULL DEFAULT 0,
    created_at     timestamptz NOT NULL DEFAULT now(),
    created_by     uuid,
    updated_at     timestamptz,
    updated_by     uuid,
    deleted_at     timestamptz,
    CONSTRAINT ck_sys_user_status CHECK (status IN ('active', 'locked', 'disabled'))
);

-- 用户名在租户内唯一；软删后允许复用同名（部分唯一索引）。
CREATE UNIQUE INDEX IF NOT EXISTS uq_sys_user_username
    ON sys_user (tenant_id, username)
    WHERE deleted_at IS NULL;

COMMENT ON TABLE sys_user IS '用户：登录主体，密码使用 Argon2id 哈希';

-- 会话表（M0 暂用进程内存储，此表为 M1 多实例部署预留）
CREATE TABLE IF NOT EXISTS sys_session (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL,
    user_id     uuid        NOT NULL,
    token_hash  text        NOT NULL,
    user_agent  text,
    ip          inet,
    expires_at  timestamptz NOT NULL,
    revoked_at  timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_sys_session_token ON sys_session (token_hash);
CREATE INDEX IF NOT EXISTS idx_sys_session_user ON sys_session (user_id, expires_at DESC);

COMMENT ON TABLE sys_session IS '会话：支持主动踢下线与多端管理（M1 启用）';

-- 审计日志：只追加，不更新不删除（M1 起改为按月分区）
CREATE TABLE IF NOT EXISTS sys_audit_log (
    id             uuid        PRIMARY KEY,
    tenant_id      uuid        NOT NULL,
    user_id        uuid,
    action         varchar(128) NOT NULL,
    resource_type  varchar(64)  NOT NULL,
    resource_id    varchar(64),
    result         varchar(16)  NOT NULL,
    ip             inet,
    user_agent     text,
    trace_id       text,
    detail         jsonb,
    created_at     timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_sys_audit_tenant_time
    ON sys_audit_log (tenant_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_sys_audit_user_time
    ON sys_audit_log (user_id, created_at DESC);

COMMENT ON TABLE sys_audit_log IS '审计日志：只追加，保留期不少于 6 个月（等保三级）';
