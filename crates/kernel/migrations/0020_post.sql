-- =============================================================================
-- 0020_post.sql —— 岗位
--
-- 岗位与部门是两个维度：部门是「组织归属」，岗位是「职责标签」。
-- 一个用户可以同时挂多个岗位（如：销售经理 + 区域负责人）。
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys_post (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL REFERENCES sys_tenant (id),
    code        varchar(64) NOT NULL,
    name        varchar(64) NOT NULL,
    sort        int         NOT NULL DEFAULT 0,
    status      varchar(16) NOT NULL DEFAULT 'active',
    remark      text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    deleted_at  timestamptz,
    CONSTRAINT ck_sys_post_status CHECK (status IN ('active', 'disabled'))
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_sys_post_code
    ON sys_post (tenant_id, code) WHERE deleted_at IS NULL;

COMMENT ON TABLE sys_post IS '岗位：职责标签，与部门维度正交';

CREATE TABLE IF NOT EXISTS sys_user_post (
    user_id uuid NOT NULL,
    post_id uuid NOT NULL REFERENCES sys_post (id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, post_id)
);

COMMENT ON TABLE sys_user_post IS '用户-岗位：多对多';

INSERT INTO sys_permission (id, plugin, code, name) VALUES
    ('00000000-0000-7000-8000-00000000011b', 'system', 'system:post:list',   '岗位查询'),
    ('00000000-0000-7000-8000-00000000011c', 'system', 'system:post:add',    '岗位新增'),
    ('00000000-0000-7000-8000-00000000011d', 'system', 'system:post:edit',   '岗位修改'),
    ('00000000-0000-7000-8000-00000000011e', 'system', 'system:post:delete', '岗位删除')
ON CONFLICT (code) DO NOTHING;

-- 新表策略（不 FORCE，与导出任务一致：先观察）
ALTER TABLE sys_post ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys_post;
CREATE POLICY tenant_isolation ON sys_post
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);
