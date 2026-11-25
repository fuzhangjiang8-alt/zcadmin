-- =============================================================================
-- force_rls.sql —— 正式启用行级安全（运维脚本，不在自动迁移里执行）
--
-- 注意：FORCE 已由迁移自动执行（0015 起），本脚本仅用于
--       「恢复备份后重新启用」「排障后恢复强制隔离」等运维场景。
--
-- 选择规则（而不是手写表名）：
--   1) 已 ENABLE RLS；
--   2) 至少存在一条策略（否则 FORCE 会导致该表完全不可读）；
--   3) schema ∈ sys / crm / demo。
-- 这样新增表会自动纳入，不会出现「迁移加了、运维脚本忘了」的漂移。
--
-- 执行：
--   psql "$DATABASE_URL" -f ops/force_rls.sql
--
-- 回滚：
--   psql "$DATABASE_URL" -f ops/disable_rls.sql
-- =============================================================================

DO $$
DECLARE
    r RECORD;
    forced int := 0;
BEGIN
    FOR r IN
        SELECT n.nspname AS schema_name, c.relname AS table_name
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE c.relkind = 'r'
          AND c.relrowsecurity
          AND NOT c.relforcerowsecurity
          AND n.nspname IN ('sys', 'crm', 'demo')
          AND EXISTS (
              SELECT 1 FROM pg_policy p WHERE p.polrelid = c.oid
          )
        ORDER BY 1, 2
    LOOP
        EXECUTE format(
            'ALTER TABLE %I.%I FORCE ROW LEVEL SECURITY',
            r.schema_name, r.table_name
        );
        forced := forced + 1;
        RAISE NOTICE 'RLS FORCE enabled: %.%', r.schema_name, r.table_name;
    END LOOP;
    RAISE NOTICE 'total forced: %', forced;
END $$;
