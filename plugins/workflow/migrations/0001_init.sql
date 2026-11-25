-- =============================================================================
-- workflow 插件初始迁移（企业级轻量审批流）
--
-- 执行上下文：`SET search_path TO workflow` 之后。
-- 铁律 7：包含 tenant_id / created_at / created_by / deleted_at / version，
--         并同时启用 ENABLE + FORCE ROW LEVEL SECURITY。
-- =============================================================================

CREATE TABLE IF NOT EXISTS workflow_request (
    id              uuid            PRIMARY KEY,
    tenant_id       uuid            NOT NULL,
    code            varchar(64)     NOT NULL,
    title           varchar(128)    NOT NULL,
    biz_type        varchar(32)     NOT NULL DEFAULT 'general',
    biz_id          varchar(64),
    status          varchar(16)     NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'approved', 'rejected', 'cancelled')),
    amount_cents    bigint          NOT NULL DEFAULT 0 CHECK (amount_cents >= 0),
    remark          text,
    decision_note   text,
    decided_by      uuid,
    decided_at      timestamptz,
    owner_id        uuid            NOT NULL,
    dept_id         uuid,
    created_at      timestamptz     NOT NULL DEFAULT now(),
    created_by      uuid            NOT NULL,
    deleted_at      timestamptz,
    version         integer         NOT NULL DEFAULT 1
);

-- 租户内未删除审批单编号唯一
CREATE UNIQUE INDEX IF NOT EXISTS uq_workflow_request_tenant_code
    ON workflow_request (tenant_id, code)
    WHERE deleted_at IS NULL;

-- 列表查询复合索引（匹配 WHERE tenant_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC, id）
CREATE INDEX IF NOT EXISTS idx_workflow_request_tenant_created
    ON workflow_request (tenant_id, created_at DESC, id)
    WHERE deleted_at IS NULL;

-- 启用并强制行级安全策略（防止表 owner 绕过）
ALTER TABLE workflow_request ENABLE ROW LEVEL SECURITY;
ALTER TABLE workflow_request FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON workflow_request;
CREATE POLICY tenant_isolation ON workflow_request
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

COMMENT ON TABLE workflow_request IS '企业审批单：支持合同/折扣/退款/通用申请的状态机流转';
COMMENT ON COLUMN workflow_request.tenant_id IS '所属租户（RLS 隔离键）';
COMMENT ON COLUMN workflow_request.status IS '状态：pending / approved / rejected / cancelled';
