-- =============================================================================
-- 0041_config_snapshot_diff.sql —— 导入差异详情
-- =============================================================================

ALTER TABLE sys.sys_audit_config_snapshot
    ADD COLUMN IF NOT EXISTS diff_detail jsonb;
