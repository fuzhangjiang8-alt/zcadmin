-- =============================================================================
-- 0070_rls_billing_tables.sql —— 租户账单/用量表的行级隔离
--
-- 背景：这 4 张表都有 tenant_id，但此前完全没有 RLS。
-- 它们只被平台级接口（system:tenant:*）和定时任务访问，所以没有直接暴露；
-- 但缺少数据库兜底，一旦某处查询漏写 tenant_id 就会跨租户泄漏。
--
-- 策略：
--   1) 平台超管：TenantTx 会设置 app.platform = 'on'（见 kernel/src/tenant.rs），放行全部；
--   2) 普通租户：只能看到自己的行（tenant_id = app.tenant_id）；
--   3) 后台任务（capture_usage / generate_bills）逐租户 set_config 后写入。
--
-- 回滚：
--   ALTER TABLE ... NO FORCE ROW LEVEL SECURITY;（见 ops/disable_rls.sql）
-- =============================================================================

ALTER TABLE sys.sys_tenant_bill ENABLE ROW LEVEL SECURITY;
ALTER TABLE sys.sys_tenant_bill FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_tenant_bill;
CREATE POLICY tenant_isolation ON sys.sys_tenant_bill
    USING (
        current_setting('app.platform', true) = 'on'
        OR tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        current_setting('app.platform', true) = 'on'
        OR tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid
    );

ALTER TABLE sys.sys_tenant_bill_payment ENABLE ROW LEVEL SECURITY;
ALTER TABLE sys.sys_tenant_bill_payment FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_tenant_bill_payment;
CREATE POLICY tenant_isolation ON sys.sys_tenant_bill_payment
    USING (
        current_setting('app.platform', true) = 'on'
        OR tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        current_setting('app.platform', true) = 'on'
        OR tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid
    );

ALTER TABLE sys.sys_tenant_bill_collection ENABLE ROW LEVEL SECURITY;
ALTER TABLE sys.sys_tenant_bill_collection FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_tenant_bill_collection;
CREATE POLICY tenant_isolation ON sys.sys_tenant_bill_collection
    USING (
        current_setting('app.platform', true) = 'on'
        OR tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        current_setting('app.platform', true) = 'on'
        OR tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid
    );

ALTER TABLE sys.sys_tenant_usage_snapshot ENABLE ROW LEVEL SECURITY;
ALTER TABLE sys.sys_tenant_usage_snapshot FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON sys.sys_tenant_usage_snapshot;
CREATE POLICY tenant_isolation ON sys.sys_tenant_usage_snapshot
    USING (
        current_setting('app.platform', true) = 'on'
        OR tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid
    )
    WITH CHECK (
        current_setting('app.platform', true) = 'on'
        OR tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid
    );
