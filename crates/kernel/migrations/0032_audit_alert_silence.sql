-- =============================================================================
-- 0032_audit_alert_silence.sql —— 告警静默期
--
-- 同一动作在 silence_minutes 内已经产生过告警时，不再重复生成。
-- =============================================================================

ALTER TABLE sys.sys_audit_alert_rule
    ADD COLUMN IF NOT EXISTS silence_minutes int NOT NULL DEFAULT 30;
