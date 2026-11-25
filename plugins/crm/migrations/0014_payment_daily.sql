-- =============================================================================
-- 0014_payment_daily.sql —— 回款按日汇总
--
-- 看板月度趋势原本对 payment 全表 group by（5 万行约 10-20ms/次，
-- 25 并发下 p95 600ms）。改为按日预聚合：
--
--   payment_daily (tenant_id, day) → amount_cents, cnt
--
-- 触发器与回款写入同事务，增量维护；看板只扫最近 6 个月（约 180 行）。
-- 注意：payment_daily 不含 owner/dept，数据范围不是 all 的用户回退到明细聚合。
-- =============================================================================

CREATE TABLE IF NOT EXISTS payment_daily (
    tenant_id    uuid   NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    day          date   NOT NULL,
    amount_cents bigint NOT NULL DEFAULT 0,
    cnt          bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, day)
);

-- 回填历史数据
INSERT INTO payment_daily (tenant_id, day, amount_cents, cnt)
SELECT tenant_id, COALESCE(paid_at, created_at)::date, sum(amount_cents)::bigint, count(*)::bigint
FROM payment
WHERE deleted_at IS NULL
GROUP BY tenant_id, COALESCE(paid_at, created_at)::date
ON CONFLICT (tenant_id, day) DO UPDATE
SET amount_cents = EXCLUDED.amount_cents,
    cnt = EXCLUDED.cnt;

ALTER TABLE payment_daily ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON payment_daily;
CREATE POLICY tenant_isolation ON payment_daily
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

CREATE OR REPLACE FUNCTION sync_payment_daily() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    old_day date;
    new_day date;
BEGIN
    IF TG_OP IN ('UPDATE', 'DELETE') THEN
        old_day := COALESCE(OLD.paid_at, OLD.created_at)::date;
        INSERT INTO crm.payment_daily (tenant_id, day, amount_cents, cnt)
        VALUES (OLD.tenant_id, old_day, -OLD.amount_cents, -1)
        ON CONFLICT (tenant_id, day) DO UPDATE
        SET amount_cents = crm.payment_daily.amount_cents + EXCLUDED.amount_cents,
            cnt = crm.payment_daily.cnt + EXCLUDED.cnt;
    END IF;

    IF TG_OP IN ('INSERT', 'UPDATE') THEN
        new_day := COALESCE(NEW.paid_at, NEW.created_at)::date;
        INSERT INTO crm.payment_daily (tenant_id, day, amount_cents, cnt)
        VALUES (NEW.tenant_id, new_day, NEW.amount_cents, 1)
        ON CONFLICT (tenant_id, day) DO UPDATE
        SET amount_cents = crm.payment_daily.amount_cents + EXCLUDED.amount_cents,
            cnt = crm.payment_daily.cnt + EXCLUDED.cnt;
    END IF;

    RETURN NULL;
END;
$$;

DROP TRIGGER IF EXISTS trg_sync_payment_daily ON payment;
CREATE TRIGGER trg_sync_payment_daily
    AFTER INSERT OR UPDATE OR DELETE ON payment
    FOR EACH ROW
    EXECUTE FUNCTION sync_payment_daily();
