-- =============================================================================
-- 0071_site_title_brand.sql —— 站点标题改为品牌名「卓才智能」
--
-- 背景：0009_config.sql 种子里 site.title 默认值为 'zcadmin'，已存在的租户
--       不会因修改种子而更新；本迁移幂等更新历史值，不动其它自定义标题。
-- 边界：只改值仍为 'zcadmin' 的行，管理员手动改过的标题保留。
-- 回滚：UPDATE sys.sys_config SET value = 'zcadmin' WHERE key = 'site.title';
-- =============================================================================

UPDATE sys.sys_config
SET value = '卓才智能', updated_at = now()
WHERE key = 'site.title' AND value = 'zcadmin';
