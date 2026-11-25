-- =============================================================================
-- 0011_contract_paid_cents.sql —— 合同已回款预聚合
--
-- 背景：5 万条回款时，合同回款接口每次 sum() 扫 5 万行（~12-18ms），
-- 25 并发下 p95 超过 700ms。聚合成本无法靠索引消除，改为在合同上冗余金额：
--
--   contract.paid_cents  由触发器在回款增/删/改时增量维护
--
-- 一致性：触发器与回款写入在同一事务内，天然原子。
-- =============================================================================

ALTER TABLE contract
    ADD COLUMN IF NOT EXISTS paid_cents bigint NOT NULL DEFAULT 0;

-- 回填历史数据
UPDATE contract c
SET paid_cents = COALESCE(
        (SELECT sum(p.amount_cents)
         FROM payment p
         WHERE p.contract_id = c.id AND p.deleted_at IS NULL),
        0
    );

CREATE OR REPLACE FUNCTION sync_contract_paid() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        UPDATE contract
        SET paid_cents = paid_cents + NEW.amount_cents,
            updated_at = now()
        WHERE id = NEW.contract_id;
        RETURN NULL;
    ELSIF TG_OP = 'DELETE' THEN
        UPDATE contract
        SET paid_cents = paid_cents - OLD.amount_cents,
            updated_at = now()
        WHERE id = OLD.contract_id;
        RETURN NULL;
    ELSE
        -- 允许改金额或改合同：先减旧、再加新
        UPDATE contract
        SET paid_cents = paid_cents - OLD.amount_cents,
            updated_at = now()
        WHERE id = OLD.contract_id;

        UPDATE contract
        SET paid_cents = paid_cents + NEW.amount_cents,
            updated_at = now()
        WHERE id = NEW.contract_id;
        RETURN NULL;
    END IF;
END;
$$;

DROP TRIGGER IF EXISTS trg_sync_contract_paid ON payment;
CREATE TRIGGER trg_sync_contract_paid
    AFTER INSERT OR UPDATE OR DELETE ON payment
    FOR EACH ROW
    EXECUTE FUNCTION sync_contract_paid();
