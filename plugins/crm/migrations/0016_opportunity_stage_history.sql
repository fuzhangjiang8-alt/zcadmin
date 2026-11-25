-- =============================================================================
-- 0016_opportunity_stage_history.sql —— 商机阶段变更历史
--
-- 目的：计算各阶段平均停留时长、阶段转化率（销售流程卡点分析）。
-- 记录方式：触发器在 INSERT 与 stage 变更时追加一行，只增不改。
-- =============================================================================

CREATE TABLE IF NOT EXISTS opportunity_stage_history (
    id             uuid        PRIMARY KEY,
    tenant_id      uuid        NOT NULL REFERENCES sys.sys_tenant (id) ON DELETE CASCADE,
    opportunity_id uuid        NOT NULL REFERENCES opportunity (id) ON DELETE CASCADE,
    from_stage     varchar(32),
    to_stage       varchar(32) NOT NULL,
    changed_at     timestamptz NOT NULL DEFAULT now(),
    changed_by     uuid
);

CREATE INDEX IF NOT EXISTS idx_opportunity_stage_history
    ON opportunity_stage_history (tenant_id, opportunity_id, changed_at);

-- 历史数据：把当前阶段作为初始记录（时间用商机创建时间）
INSERT INTO opportunity_stage_history
    (id, tenant_id, opportunity_id, from_stage, to_stage, changed_at, changed_by)
SELECT gen_random_uuid(), o.tenant_id, o.id, NULL, o.stage, o.created_at, o.created_by
FROM opportunity o
WHERE NOT EXISTS (
    SELECT 1 FROM opportunity_stage_history h WHERE h.opportunity_id = o.id
);

CREATE OR REPLACE FUNCTION record_opportunity_stage() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        INSERT INTO crm.opportunity_stage_history
            (id, tenant_id, opportunity_id, from_stage, to_stage, changed_at, changed_by)
        VALUES (gen_random_uuid(), NEW.tenant_id, NEW.id, NULL, NEW.stage, NEW.created_at, NEW.created_by);
    ELSIF NEW.stage IS DISTINCT FROM OLD.stage THEN
        INSERT INTO crm.opportunity_stage_history
            (id, tenant_id, opportunity_id, from_stage, to_stage, changed_at, changed_by)
        VALUES (gen_random_uuid(), NEW.tenant_id, NEW.id, OLD.stage, NEW.stage, now(), NEW.updated_by);
    END IF;
    RETURN NULL;
END;
$$;

DROP TRIGGER IF EXISTS trg_record_opportunity_stage ON opportunity;
CREATE TRIGGER trg_record_opportunity_stage
    AFTER INSERT OR UPDATE OF stage ON opportunity
    FOR EACH ROW
    EXECUTE FUNCTION record_opportunity_stage();

ALTER TABLE opportunity_stage_history ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON opportunity_stage_history;
CREATE POLICY tenant_isolation ON opportunity_stage_history
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

ALTER TABLE opportunity_stage_history FORCE ROW LEVEL SECURITY;
