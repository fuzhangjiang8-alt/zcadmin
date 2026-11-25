-- =============================================================================
-- disable_rls.sql —— 紧急回滚：关闭 FORCE（策略保留，应用重新可绕过）
--
-- 用于 FORCE 后出现「登录失败 / 列表为空」等异常时的快速恢复。
-- 执行： psql "$DATABASE_URL" -f ops/disable_rls.sql
--
-- 为什么用动态循环而不是手写表名：
-- 迁移已经加到 60+，手写清单必然落后；这里直接遍历 catalog，
-- 保证「新增表自动覆盖」，回滚脚本不会再漂移。
-- 注意：只关 FORCE，不 DISABLE RLS；普通角色仍受策略约束。
-- =============================================================================

DO $$
DECLARE
    r RECORD;
BEGIN
    FOR r IN
        SELECT n.nspname AS schema_name, c.relname AS table_name
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE c.relkind = 'r'
          AND c.relforcerowsecurity
          AND n.nspname IN ('sys', 'crm', 'demo')
        ORDER BY 1, 2
    LOOP
        EXECUTE format(
            'ALTER TABLE %I.%I NO FORCE ROW LEVEL SECURITY',
            r.schema_name, r.table_name
        );
        RAISE NOTICE 'RLS FORCE disabled: %.%', r.schema_name, r.table_name;
    END LOOP;
END $$;
