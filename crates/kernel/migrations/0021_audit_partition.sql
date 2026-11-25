-- =============================================================================
-- 0021_audit_partition.sql —— 审计日志按月分区
--
-- 目标：
--   - 查询按 created_at 裁剪，只扫相关月份；
--   - 保留策略从 DELETE 变为 DROP PARTITION（秒级、无膨胀）。
--
-- 注意：分区表的 PRIMARY KEY 必须包含分区键，因此主键改为 (id, created_at)。
-- 一次性迁移：重命名旧表 → 建分区父表 → 建历史分区 → 拷贝数据 → 删除旧表。
-- =============================================================================

ALTER TABLE sys.sys_audit_log RENAME TO sys_audit_log_legacy;

CREATE TABLE sys.sys_audit_log (
    id             uuid         NOT NULL,
    tenant_id      uuid         NOT NULL,
    user_id        uuid,
    action         varchar(128) NOT NULL,
    resource_type  varchar(64)  NOT NULL,
    resource_id    varchar(64),
    result         varchar(16)  NOT NULL,
    ip             inet,
    user_agent     text,
    trace_id       text,
    detail         jsonb,
    created_at     timestamptz  NOT NULL DEFAULT now(),
    PRIMARY KEY (id, created_at)
) PARTITION BY RANGE (created_at);

-- 为历史数据所在月份到未来 2 个月建分区
DO $$
DECLARE
    start_month date;
    m date;
BEGIN
    SELECT date_trunc('month', COALESCE(min(created_at), now()))::date
      INTO start_month
      FROM sys.sys_audit_log_legacy;

    m := start_month;
    WHILE m <= (date_trunc('month', now())::date + interval '2 months') LOOP
        EXECUTE format(
            'CREATE TABLE IF NOT EXISTS sys.sys_audit_log_%s PARTITION OF sys.sys_audit_log FOR VALUES FROM (%L) TO (%L)',
            to_char(m, 'YYYYMM'),
            m,
            m + interval '1 month'
        );
        m := m + interval '1 month';
    END LOOP;

    -- 兜底分区：防止未来月份未及时建分区导致插入失败
    EXECUTE 'CREATE TABLE IF NOT EXISTS sys.sys_audit_log_default PARTITION OF sys.sys_audit_log DEFAULT';
END $$;

INSERT INTO sys.sys_audit_log
    (id, tenant_id, user_id, action, resource_type, resource_id, result,
     ip, user_agent, trace_id, detail, created_at)
SELECT id, tenant_id, user_id, action, resource_type, resource_id, result,
       ip, user_agent, trace_id, detail, created_at
FROM sys.sys_audit_log_legacy;

DROP TABLE sys.sys_audit_log_legacy;

CREATE INDEX idx_sys_audit_tenant_time
    ON sys.sys_audit_log (tenant_id, created_at DESC);
CREATE INDEX idx_sys_audit_user_time
    ON sys.sys_audit_log (user_id, created_at DESC);

ALTER TABLE sys.sys_audit_log ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_audit_log;
CREATE POLICY tenant_isolation ON sys.sys_audit_log
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_audit_log FORCE ROW LEVEL SECURITY;
