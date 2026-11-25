-- =============================================================================
-- ops/init-app-role.sql —— 应用专用数据库角色（仅首次初始化时执行）
--
-- 为什么需要它：
--   docker 官方 postgres 镜像会把 POSTGRES_USER 建成超级用户，而超级用户
--   永远绕过 RLS —— 这会让 FORCE ROW LEVEL SECURITY 形同虚设。
--
-- 做法：
--   1) 管理员连接（zcadmin）只用于建库/运维，不承载业务查询；
--   2) 应用使用 zcadmin_app（NOSUPERUSER NOBYPASSRLS）连接；
--   3) 把数据库 owner 转给应用角色，让迁移能建 schema / 表。
--
-- 注意：本脚本只在**空数据卷**首次初始化时执行。已有数据卷请手动执行，
--       或 `docker compose down -v` 后重建（会清空开发数据）。
-- =============================================================================

CREATE ROLE zcadmin_app LOGIN PASSWORD 'zcadmin_app' NOSUPERUSER NOBYPASSRLS;

ALTER DATABASE zcadmin OWNER TO zcadmin_app;
GRANT ALL PRIVILEGES ON DATABASE zcadmin TO zcadmin_app;

COMMENT ON ROLE zcadmin_app IS 'zcadmin 应用连接角色：非超级用户、不绕过 RLS';
