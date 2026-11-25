-- =============================================================================
-- 回款（由 cargo xtask gen 生成）
--
-- 注意：本文件生成后即可自由修改；已发布后只能追加新迁移。
-- =============================================================================

CREATE TABLE IF NOT EXISTS payment (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL,
    contract_id uuid NOT NULL ,
    payment_no varchar(64) NOT NULL ,
    amount_cents int NOT NULL ,
    paid_at timestamptz ,
    method varchar(32) ,
    owner_id uuid NOT NULL ,
    dept_id uuid ,
    version     bigint      NOT NULL DEFAULT 0,
    created_at  timestamptz NOT NULL DEFAULT now(),
    created_by  uuid,
    updated_at  timestamptz,
    updated_by  uuid,
    deleted_at  timestamptz
);

COMMENT ON TABLE payment IS '回款';

CREATE INDEX IF NOT EXISTS idx_payment_list
    ON payment (tenant_id, created_at DESC, id)
    WHERE deleted_at IS NULL;

ALTER TABLE payment ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON payment;
CREATE POLICY tenant_isolation ON payment
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);
ALTER TABLE payment FORCE ROW LEVEL SECURITY;
