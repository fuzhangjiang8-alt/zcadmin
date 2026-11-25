-- =============================================================================
-- 商机（由 cargo xtask gen 生成）
--
-- 注意：本文件生成后即可自由修改；已发布后只能追加新迁移。
-- =============================================================================

CREATE TABLE IF NOT EXISTS opportunity (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL,
    customer_id uuid NOT NULL ,
    name varchar(128) NOT NULL ,
    amount_cents int ,
    stage varchar(32) NOT NULL ,
    expected_close timestamptz ,
    owner_id uuid NOT NULL ,
    dept_id uuid ,
    version     bigint      NOT NULL DEFAULT 0,
    created_at  timestamptz NOT NULL DEFAULT now(),
    created_by  uuid,
    updated_at  timestamptz,
    updated_by  uuid,
    deleted_at  timestamptz
);

COMMENT ON TABLE opportunity IS '商机';

CREATE INDEX IF NOT EXISTS idx_opportunity_list
    ON opportunity (tenant_id, created_at DESC, id)
    WHERE deleted_at IS NULL;

-- RLS：生成器只建表，策略需要显式补
ALTER TABLE opportunity ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON opportunity;
CREATE POLICY tenant_isolation ON opportunity
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

-- 生成的 handler 使用 TenantTx，可以安全 FORCE
ALTER TABLE opportunity FORCE ROW LEVEL SECURITY;
