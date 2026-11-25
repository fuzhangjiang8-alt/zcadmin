-- =============================================================================
-- 0072_site_title_seed.sql —— 补全/更新站点标题为品牌名「卓才智能」
--
-- 背景：0009 种子写入 'zcadmin'，部分早期库缺该行；0071 的 UPDATE 在无
--       app.tenant_id 上下文的迁移连接里受 FORCE RLS 限制，影响 0 行。
--       本迁移逐租户设置 RLS 上下文后再更新 + 补插，幂等。
-- 约束：只改值仍为 'zcadmin' 的行，管理员自定义标题保留；只补确实缺行的租户。
-- 回滚：DELETE FROM sys.sys_config WHERE key='site.title' AND value='卓才智能';
-- =============================================================================

DO $$
DECLARE
    r RECORD;
BEGIN
    FOR r IN SELECT id FROM sys.sys_tenant WHERE deleted_at IS NULL LOOP
        -- 迁移连接没有请求级事务，必须显式设置租户上下文才能通过 FORCE RLS
        PERFORM set_config('app.tenant_id', r.id::text, true);

        UPDATE sys.sys_config
        SET value = '卓才智能', updated_at = now()
        WHERE tenant_id = r.id AND key = 'site.title' AND value = 'zcadmin';

        INSERT INTO sys.sys_config (id, tenant_id, key, value, name, remark, is_public)
        SELECT gen_random_uuid(), r.id, 'site.title', '卓才智能',
               '站点标题', '浏览器标题与登录页标题', true
        WHERE NOT EXISTS (
            SELECT 1 FROM sys.sys_config c
            WHERE c.tenant_id = r.id AND c.key = 'site.title' AND c.deleted_at IS NULL
        );
    END LOOP;
END $$;
