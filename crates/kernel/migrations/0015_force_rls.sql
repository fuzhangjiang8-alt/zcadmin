-- =============================================================================
-- 0015_force_rls.sql —— 正式启用行级安全（内核表）
--
-- 经过 2026-10 真机冒烟验证（登录 / 全部系统读写 / 文件上传 / CRM CRUD），
-- 所有访问路径都通过 TenantTx 注入 app.tenant_id，因此可以 FORCE。
--
-- 回滚：执行 ops/disable_rls.sql（或对单表 NO FORCE）。
-- =============================================================================

ALTER TABLE sys_config        FORCE ROW LEVEL SECURITY;
ALTER TABLE sys_notice        FORCE ROW LEVEL SECURITY;
ALTER TABLE sys_notice_read   FORCE ROW LEVEL SECURITY;
ALTER TABLE sys_dept          FORCE ROW LEVEL SECURITY;
ALTER TABLE sys_role          FORCE ROW LEVEL SECURITY;
ALTER TABLE sys_dict_type     FORCE ROW LEVEL SECURITY;
ALTER TABLE sys_dict_item     FORCE ROW LEVEL SECURITY;
ALTER TABLE sys_audit_log     FORCE ROW LEVEL SECURITY;
ALTER TABLE sys_file          FORCE ROW LEVEL SECURITY;
ALTER TABLE sys_user          FORCE ROW LEVEL SECURITY;
ALTER TABLE sys_menu          FORCE ROW LEVEL SECURITY;
