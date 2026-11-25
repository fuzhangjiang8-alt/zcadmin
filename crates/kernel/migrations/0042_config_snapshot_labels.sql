-- =============================================================================
-- 0042_config_snapshot_labels.sql —— 快照名称/备注
-- =============================================================================

ALTER TABLE sys.sys_audit_config_snapshot
    ADD COLUMN IF NOT EXISTS name text,
    ADD COLUMN IF NOT EXISTS note text;
