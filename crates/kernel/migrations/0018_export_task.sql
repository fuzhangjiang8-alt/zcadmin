-- =============================================================================
-- 0018_export_task.sql —— 异步导出任务
--
-- 同步导出的问题是「请求超时」：数据量一大，HTTP 连接先断。
-- 异步任务把「生成」与「下载」拆开：创建任务 → 后台生成 → 轮询状态 → 下载文件。
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys_export_task (
    id           uuid        PRIMARY KEY,
    tenant_id    uuid        NOT NULL REFERENCES sys_tenant (id),
    user_id      uuid        NOT NULL,
    kind         varchar(64) NOT NULL,
    keyword      varchar(128),
    status       varchar(16) NOT NULL DEFAULT 'pending',
    file_key     text,
    file_name    varchar(255),
    total_rows   bigint,
    error        text,
    created_at   timestamptz NOT NULL DEFAULT now(),
    started_at   timestamptz,
    finished_at  timestamptz,
    CONSTRAINT ck_export_status CHECK (status IN ('pending', 'running', 'success', 'failed'))
);

CREATE INDEX IF NOT EXISTS idx_export_task_user
    ON sys_export_task (tenant_id, user_id, created_at DESC);

COMMENT ON TABLE sys_export_task IS '异步导出任务：后台生成文件，前端轮询状态后下载';

INSERT INTO sys_permission (id, plugin, code, name) VALUES
    ('00000000-0000-7000-8000-000000000117', 'system', 'system:export:list', '导出任务查询')
ON CONFLICT (code) DO NOTHING;

-- 策略先建、暂不 FORCE：启动时的 recover_stale 需要跨租户把中断任务标记失败
ALTER TABLE sys_export_task ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys_export_task;
CREATE POLICY tenant_isolation ON sys_export_task
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);
