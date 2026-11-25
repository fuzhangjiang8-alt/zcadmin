-- =============================================================================
-- 0009_config.sql —— 参数配置
--
-- 与字典的区别：
--   字典 = 一组枚举值（下拉框用）
--   参数 = 单个键值对（系统行为开关、标题、分页大小等）
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys_config (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL REFERENCES sys_tenant (id),
    key         varchar(128) NOT NULL,
    value       text        NOT NULL,
    name        varchar(128) NOT NULL,
    remark      text,
    -- 是否允许前端读取（例如站点标题）；敏感配置（密钥）必须为 false
    is_public   boolean     NOT NULL DEFAULT false,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz,
    deleted_at  timestamptz
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_sys_config_key
    ON sys_config (tenant_id, key) WHERE deleted_at IS NULL;

COMMENT ON TABLE sys_config IS '参数配置：键值对形式的系统参数';

INSERT INTO sys_permission (id, plugin, code, name) VALUES
    ('00000000-0000-7000-8000-000000000112', 'system', 'system:config:list', '参数查询'),
    ('00000000-0000-7000-8000-000000000113', 'system', 'system:config:edit', '参数修改')
ON CONFLICT (code) DO NOTHING;

-- 示例参数（幂等）
INSERT INTO sys_config (id, tenant_id, key, value, name, remark, is_public)
SELECT gen_random_uuid(), t.id, 'site.title', 'zcadmin', '站点标题', '浏览器标题与登录页标题', true
FROM sys_tenant t
WHERE NOT EXISTS (SELECT 1 FROM sys_config c WHERE c.tenant_id = t.id AND c.key = 'site.title');
