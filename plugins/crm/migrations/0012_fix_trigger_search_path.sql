-- =============================================================================
-- 0012_fix_trigger_search_path.sql
--
-- 0011 的触发器函数里写的是未限定表名 `contract`，函数体在运行期按调用者的
-- search_path 解析；应用连接默认 search_path 不含 crm，导致
-- `relation "contract" does not exist`（42P01）。
--
-- 修复：函数内全部使用 schema 限定名。
-- =============================================================================

CREATE OR REPLACE FUNCTION sync_contract_paid() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        UPDATE crm.contract
        SET paid_cents = paid_cents + NEW.amount_cents,
            updated_at = now()
        WHERE id = NEW.contract_id;
        RETURN NULL;
    ELSIF TG_OP = 'DELETE' THEN
        UPDATE crm.contract
        SET paid_cents = paid_cents - OLD.amount_cents,
            updated_at = now()
        WHERE id = OLD.contract_id;
        RETURN NULL;
    ELSE
        UPDATE crm.contract
        SET paid_cents = paid_cents - OLD.amount_cents,
            updated_at = now()
        WHERE id = OLD.contract_id;

        UPDATE crm.contract
        SET paid_cents = paid_cents + NEW.amount_cents,
            updated_at = now()
        WHERE id = NEW.contract_id;
        RETURN NULL;
    END IF;
END;
$$;
