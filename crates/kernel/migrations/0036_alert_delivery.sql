-- =============================================================================
-- 0036_alert_delivery.sql —— 按订阅的多目标 Webhook 投递
-- =============================================================================

CREATE TABLE IF NOT EXISTS sys.sys_audit_alert_subscription_config (
    tenant_id  uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    user_id    uuid        NOT NULL REFERENCES sys.sys_user (id) ON DELETE CASCADE,
    url        text        NOT NULL DEFAULT '',
    secret     text,
    enabled    boolean     NOT NULL DEFAULT false,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, user_id)
);

ALTER TABLE sys.sys_audit_alert_subscription_config ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_audit_alert_subscription_config;
CREATE POLICY tenant_isolation ON sys.sys_audit_alert_subscription_config
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_audit_alert_subscription_config FORCE ROW LEVEL SECURITY;

CREATE TABLE IF NOT EXISTS sys.sys_audit_alert_delivery (
    id              uuid        PRIMARY KEY,
    tenant_id       uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    alert_id        uuid        NOT NULL REFERENCES sys.sys_audit_alert (id) ON DELETE CASCADE,
    subscription_id uuid        NOT NULL REFERENCES sys.sys_audit_alert_subscription (id) ON DELETE CASCADE,
    url             text        NOT NULL,
    secret          text,
    status          text        NOT NULL DEFAULT 'pending',
    attempts        int         NOT NULL DEFAULT 0,
    next_attempt_at timestamptz,
    sent_at         timestamptz,
    last_error      text,
    created_at      timestamptz NOT NULL DEFAULT now(),
    UNIQUE (alert_id, subscription_id)
);

CREATE INDEX IF NOT EXISTS idx_audit_alert_delivery_pending
    ON sys.sys_audit_alert_delivery (tenant_id, next_attempt_at)
    WHERE status = 'pending';

ALTER TABLE sys.sys_audit_alert_delivery ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_audit_alert_delivery;
CREATE POLICY tenant_isolation ON sys.sys_audit_alert_delivery
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE sys.sys_audit_alert_delivery FORCE ROW LEVEL SECURITY;
