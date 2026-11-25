-- =============================================================================
-- 0027_audit_alert.sql —— 审计异常告警
--
-- 规则：5 分钟窗口内同一动作出现次数 >= 50 时生成一条告警。
-- 由后台任务周期扫描写入（见 kernel::audit::detect_alerts）。
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_audit_alert (
    id              uuid        PRIMARY KEY,
    tenant_id       uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    action          text        NOT NULL,
    window_start    timestamptz NOT NULL,
    window_end      timestamptz NOT NULL,
    count           bigint      NOT NULL,
    threshold       int         NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    acknowledged_at timestamptz,
    acknowledged_by uuid
);

CREATE UNIQUE INDEX IF NOT EXISTS uq_sys_audit_alert_window
    ON sys.sys_audit_alert (tenant_id, action, window_start);

CREATE INDEX IF NOT EXISTS idx_sys_audit_alert_tenant
    ON sys.sys_audit_alert (tenant_id, created_at DESC);

ALTER TABLE sys.sys_audit_alert ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_audit_alert;
CREATE POLICY tenant_isolation ON sys.sys_audit_alert
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_audit_alert FORCE ROW LEVEL SECURITY;

INSERT INTO sys.sys_permission (id, plugin, code, name) VALUES
    ('00000000-0000-7000-8000-000000000b03', 'system', 'system:audit:alert', '审计告警处理')
ON CONFLICT (code) DO NOTHING;
