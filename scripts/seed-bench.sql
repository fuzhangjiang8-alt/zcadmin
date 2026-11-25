-- =============================================================================
-- seed-bench.sql —— 造压测数据（仅开发/测试库使用）
--
-- 生成：
--   - 1 个固定合同（id 00000000-0000-7000-8000-00000000c001）
--   - 50000 条回款
--
-- 执行：
--   psql "$DATABASE_URL" -f scripts/seed-bench.sql
-- 然后：
--   $env:BENCH_CONTRACT_ID="00000000-0000-7000-8000-00000000c001"
--   cargo test -p server --test bench -- --ignored --nocapture
--
-- 注意：脚本会先清理同一合同的旧回款，可重复执行。
-- =============================================================================

BEGIN;

DO $$
DECLARE
    t_id uuid;
    c_id uuid;
    u_id uuid;
    ct_id uuid := '00000000-0000-7000-8000-00000000c001';
BEGIN
    SELECT id INTO t_id FROM sys.sys_tenant LIMIT 1;
    IF t_id IS NULL THEN
        RAISE EXCEPTION '没有租户，请先启动一次服务完成 seed';
    END IF;

    PERFORM set_config('app.tenant_id', t_id::text, true);

    SELECT id INTO c_id FROM crm.customer WHERE tenant_id = t_id LIMIT 1;
    IF c_id IS NULL THEN
        INSERT INTO crm.customer (id, tenant_id, code, name, level, status)
        VALUES (gen_random_uuid(), t_id, 'BENCH-SEED', 'Bench Seed Customer', 'normal', 'potential')
        RETURNING id INTO c_id;
    END IF;

    SELECT id INTO u_id FROM sys.sys_user WHERE tenant_id = t_id LIMIT 1;

    INSERT INTO crm.contract
        (id, tenant_id, customer_id, contract_no, name, amount_cents, status, owner_id)
    VALUES
        (ct_id, t_id, c_id, 'BENCH-CT-1', 'Bench Contract', 5000000, 'active', u_id)
    ON CONFLICT (id) DO NOTHING;

    DELETE FROM crm.payment WHERE contract_id = ct_id;

    -- 批量造数时关闭增量触发器，最后一次性回填，避免 5 万次单行 UPDATE
    ALTER TABLE crm.payment DISABLE TRIGGER trg_sync_contract_paid;
    ALTER TABLE crm.payment DISABLE TRIGGER trg_sync_payment_daily;

    INSERT INTO crm.payment
        (id, tenant_id, contract_id, payment_no, amount_cents, paid_at, method, owner_id)
    SELECT gen_random_uuid(), t_id, ct_id, 'BENCH-HK-' || g, 100,
           now() - (g || ' minutes')::interval, 'transfer', u_id
    FROM generate_series(1, 50000) AS g;

    ALTER TABLE crm.payment ENABLE TRIGGER trg_sync_contract_paid;
    ALTER TABLE crm.payment ENABLE TRIGGER trg_sync_payment_daily;
    UPDATE crm.contract SET paid_cents = 5000000 WHERE id = ct_id;

    DELETE FROM crm.payment_daily WHERE tenant_id = t_id;
    INSERT INTO crm.payment_daily (tenant_id, day, amount_cents, cnt)
    SELECT tenant_id, COALESCE(paid_at, created_at)::date, sum(amount_cents)::bigint, count(*)::bigint
    FROM crm.payment
    WHERE tenant_id = t_id AND deleted_at IS NULL
    GROUP BY tenant_id, COALESCE(paid_at, created_at)::date;

    RAISE NOTICE 'seeded contract %, payments: 50000', ct_id;
END $$;

COMMIT;
