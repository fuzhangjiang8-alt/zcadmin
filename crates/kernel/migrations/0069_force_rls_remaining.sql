-- =============================================================================
-- 0069_force_rls_remaining.sql —— 补齐最后两张「只 ENABLE 未 FORCE」的内核表
--
-- 背景：审计发现 sys_export_task / sys_post 只 ENABLE、未 FORCE，
-- 表 owner（应用连接）可以绕过策略。0068 之前它们被排除是因为：
--   - sys_export_task：启动时 recover_stale 需要跨租户收尾；
--   - sys_post：历史迁移遗漏。
-- 现在 recover_stale 已改为逐租户设置上下文，可以安全 FORCE。
--
-- crm.payment_daily 属于插件 schema，在内核迁移阶段可能尚不存在，
-- 由 plugins/crm/migrations/0018_force_rls_payment_daily.sql 处理。
-- =============================================================================

ALTER TABLE sys.sys_export_task FORCE ROW LEVEL SECURITY;
ALTER TABLE sys.sys_post FORCE ROW LEVEL SECURITY;
