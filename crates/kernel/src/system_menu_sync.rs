//! 插件菜单同步：把插件 `MenuDef` 幂等写入每个租户的菜单表。
//!
//! 边界：只处理菜单同步；菜单 CRUD 在 `system_menus_admin`。
//! 关键约束：按「租户 × 路径」判重，父菜单通过路径解析。

use uuid::Uuid;

/// 同步插件声明的菜单（幂等）。
///
/// 按「租户 × 路径」判断是否已存在；父菜单通过路径解析。
/// 这样插件升级新增菜单时，老租户下次启动会自动补上。
pub async fn sync_plugin_menus(
    pool: &sqlx::PgPool,
    menus: &[crate::plugin::MenuDef],
) -> anyhow::Result<()> {
    let tenants: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM sys.sys_tenant WHERE deleted_at IS NULL")
            .fetch_all(pool)
            .await?;

    for tenant_id in tenants {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await?;

        for m in menus {
            sqlx::query(
                r#"
                INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
                SELECT gen_random_uuid(), $1,
                       (SELECT p.id FROM sys.sys_menu p WHERE p.tenant_id = $1 AND p.path = $6),
                       $2, $3, $4, $5, $7
                WHERE NOT EXISTS (
                    SELECT 1 FROM sys.sys_menu x WHERE x.tenant_id = $1 AND x.path = $3
                )
                "#,
            )
            .bind(tenant_id)
            .bind(m.title)
            .bind(m.path)
            .bind(m.icon)
            .bind(m.perm)
            .bind(m.parent)
            .bind(m.sort)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
    }
    Ok(())
}
