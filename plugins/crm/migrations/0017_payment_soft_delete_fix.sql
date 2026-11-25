-- =============================================================================
-- 0017_payment_soft_delete_fix.sql —— 回款软删必须冲减预聚合
--
-- 背景（缺陷）：
--   0011/0014 的触发器把任何 UPDATE 都当成「先减旧、再加新」。
--   而软删就是 `UPDATE payment SET deleted_at = now()`，金额不变、deleted_at 变了，
--   触发器净效果为 0 —— 于是 contract.paid_cents 与 payment_daily 永久虚增。
--
-- 修复：
--   触发器只统计 `deleted_at IS NULL` 的有效行：
--     INSERT 有效行 → +
--     UPDATE 有效→无效 → -；无效→有效 → +
--     DELETE 有效行 → -
--
-- 同时回填历史漂移：contract.paid_cents 重算、payment_daily 重建。
-- =============================================================================

CREATE OR REPLACE FUNCTION crm.sync_contract_paid() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NEW.deleted_at IS NULL THEN
            UPDATE crm.contract
            SET paid_cents = paid_cents + NEW.amount_cents,
                updated_at = now()
            WHERE id = NEW.contract_id;
        END IF;
        RETURN NULL;
    ELSIF TG_OP = 'DELETE' THEN
        IF OLD.deleted_at IS NULL THEN
            UPDATE crm.contract
            SET paid_cents = paid_cents - OLD.amount_cents,
                updated_at = now()
            WHERE id = OLD.contract_id;
        END IF;
        RETURN NULL;
    ELSE
        IF OLD.deleted_at IS NULL THEN
            UPDATE crm.contract
            SET paid_cents = paid_cents - OLD.amount_cents,
                updated_at = now()
            WHERE id = OLD.contract_id;
        END IF;
        IF NEW.deleted_at IS NULL THEN
            UPDATE crm.contract
            SET paid_cents = paid_cents + NEW.amount_cents,
                updated_at = now()
            WHERE id = NEW.contract_id;
        END IF;
        RETURN NULL;
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION crm.sync_payment_daily() RETURNS trigger
LANGUAGE plpgsql AS $$
DECLARE
    old_day date;
    new_day date;
BEGIN
    IF TG_OP IN ('UPDATE', 'DELETE') AND OLD.deleted_at IS NULL THEN
        old_day := COALESCE(OLD.paid_at, OLD.created_at)::date;
        INSERT INTO crm.payment_daily (tenant_id, day, amount_cents, cnt)
        VALUES (OLD.tenant_id, old_day, -OLD.amount_cents, -1)
        ON CONFLICT (tenant_id, day) DO UPDATE
        SET amount_cents = crm.payment_daily.amount_cents + EXCLUDED.amount_cents,
            cnt = crm.payment_daily.cnt + EXCLUDED.cnt;
    END IF;

    IF TG_OP IN ('INSERT', 'UPDATE') AND NEW.deleted_at IS NULL THEN
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

-- 回填：合同已回款金额以有效回款为准。
-- contract / payment 都是 FORCE RLS，迁移没有租户上下文，
-- 必须逐租户 set_config 后再重算（否则 UPDATE 匹配 0 行且不报错）。
DO $$
DECLARE
    t RECORD;
BEGIN
    FOR t IN SELECT id FROM sys.sys_tenant LOOP
        PERFORM set_config('app.tenant_id', t.id::text, true);

        UPDATE crm.contract c
        SET paid_cents = COALESCE(
                (SELECT sum(p.amount_cents)
                 FROM crm.payment p
                 WHERE p.contract_id = c.id AND p.deleted_at IS NULL),
                0
            ),
            updated_at = now()
        WHERE c.tenant_id = t.id;

        -- 按日汇总整体重建（数据量小，直接重算最可靠）
        DELETE FROM crm.payment_daily WHERE tenant_id = t.id;
        INSERT INTO crm.payment_daily (tenant_id, day, amount_cents, cnt)
        SELECT tenant_id,
               COALESCE(paid_at, created_at)::date,
               sum(amount_cents)::bigint,
               count(*)::bigint
        FROM crm.payment
        WHERE deleted_at IS NULL AND tenant_id = t.id
        GROUP BY tenant_id, COALESCE(paid_at, created_at)::date;
    END LOOP;
END $$;
