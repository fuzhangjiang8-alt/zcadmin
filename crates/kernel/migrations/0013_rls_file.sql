-- =============================================================================
-- 0013_rls_file.sql —— RLS 策略：sys_file
-- 不 FORCE：与 0010/0011/0012 保持一致，等 FORCE 阶段统一启用。
-- =============================================================================

ALTER TABLE sys_file ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS tenant_isolation ON sys_file;
CREATE POLICY tenant_isolation ON sys_file
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);
