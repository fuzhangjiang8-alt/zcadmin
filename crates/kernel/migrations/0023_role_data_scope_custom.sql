-- =============================================================================
-- 0023_role_data_scope_custom.sql
--
-- 0005 的检查约束只允许 all/dept_and_child/dept/self，
-- 新增 custom 后必须同步约束，否则创建自定义范围角色会 500。
-- =============================================================================

ALTER TABLE sys.sys_role DROP CONSTRAINT IF EXISTS ck_sys_role_data_scope;

ALTER TABLE sys.sys_role ADD CONSTRAINT ck_sys_role_data_scope
    CHECK (data_scope IN ('all', 'dept_and_child', 'custom', 'dept', 'self'));

COMMENT ON COLUMN sys.sys_role.data_scope IS '数据范围：all/dept_and_child/custom/dept/self';
