-- =============================================================================
-- 0030_audit_alert_rules.sql —— 按动作配置告警阈值
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_audit_alert_rule (
    id             uuid        PRIMARY KEY,
    tenant_id      uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    action_pattern text        NOT NULL,
    window_minutes int         NOT NULL DEFAULT 5,
    threshold      int         NOT NULL DEFAULT 50,
    enabled        boolean     NOT NULL DEFAULT true,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, action_pattern)
);

-- 默认规则：通配规则沿用现有租户配置；登录失败/导入使用更敏感阈值
INSERT INTO sys.sys_audit_alert_rule
    (id, tenant_id, action_pattern, window_minutes, threshold)
SELECT gen_random_uuid(), c.tenant_id, '*', c.window_minutes, c.threshold
FROM sys.sys_audit_alert_config c
ON CONFLICT (tenant_id, action_pattern) DO NOTHING;

INSERT INTO sys.sys_audit_alert_rule
    (id, tenant_id, action_pattern, window_minutes, threshold)
SELECT gen_random_uuid(), t.id, 'auth.login.failed', 5, 20
FROM sys.sys_tenant t
ON CONFLICT (tenant_id, action_pattern) DO NOTHING;

INSERT INTO sys.sys_audit_alert_rule
    (id, tenant_id, action_pattern, window_minutes, threshold)
SELECT gen_random_uuid(), t.id, 'system.import', 5, 10
FROM sys.sys_tenant t
ON CONFLICT (tenant_id, action_pattern) DO NOTHING;

ALTER TABLE sys.sys_audit_alert_rule ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_audit_alert_rule;
CREATE POLICY tenant_isolation ON sys.sys_audit_alert_rule
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_audit_alert_rule FORCE ROW LEVEL SECURITY;
