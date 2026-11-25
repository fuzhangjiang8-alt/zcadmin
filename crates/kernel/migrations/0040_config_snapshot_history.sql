-- =============================================================================
-- 0040_config_snapshot_history.sql —— 导入历史元数据
-- =============================================================================

ALTER TABLE sys.sys_audit_config_snapshot
    ADD COLUMN IF NOT EXISTS sections text,
    ADD COLUMN IF NOT EXISTS diff_summary jsonb;
