-- =============================================================================
-- 合同（由 cargo xtask gen 生成）
--
-- 注意：本文件生成后即可自由修改；已发布后只能追加新迁移。
-- =============================================================================

CREATE TABLE IF NOT EXISTS contract (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL,
    customer_id uuid NOT NULL ,
    contract_no varchar(64) NOT NULL ,
    name varchar(128) NOT NULL ,
    amount_cents int NOT NULL ,
    status varchar(32) NOT NULL ,
    signed_at timestamptz ,
    owner_id uuid NOT NULL ,
    dept_id uuid ,
    version     bigint      NOT NULL DEFAULT 0,
    created_at  timestamptz NOT NULL DEFAULT now(),
    created_by  uuid,
    updated_at  timestamptz,
    updated_by  uuid,
    deleted_at  timestamptz
);

COMMENT ON TABLE contract IS '合同';

CREATE INDEX IF NOT EXISTS idx_contract_list
    ON contract (tenant_id, created_at DESC, id)
    WHERE deleted_at IS NULL;

ALTER TABLE contract ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON contract;
CREATE POLICY tenant_isolation ON contract
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);
ALTER TABLE contract FORCE ROW LEVEL SECURITY;
