-- =============================================================================
-- 0011_rls_notice.sql —— RLS 策略：sys_notice / sys_notice_read
--
-- 与 0010 同理：先建策略，**不 FORCE**，等代码全部使用 TenantTx 后再启用。
-- =============================================================================

ALTER TABLE sys_notice ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS tenant_isolation ON sys_notice;
CREATE POLICY tenant_isolation ON sys_notice
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

-- 已读表没有 tenant_id，通过通知 id 间接约束
ALTER TABLE sys_notice_read ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS tenant_isolation ON sys_notice_read;
CREATE POLICY tenant_isolation ON sys_notice_read
    USING (EXISTS (
        SELECT 1 FROM sys_notice n
        WHERE n.id = sys_notice_read.notice_id
          AND n.tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid
    ))
    WITH CHECK (EXISTS (
        SELECT 1 FROM sys_notice n
        WHERE n.id = sys_notice_read.notice_id
          AND n.tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid
    ));
