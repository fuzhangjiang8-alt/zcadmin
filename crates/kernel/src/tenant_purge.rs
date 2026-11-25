//! 已删除租户的物理清理。
//!
//! 只清理 `deleted_at` 超过保留期的租户；按外键依赖从叶子表向父表删除，
//! 最后删除 `sys_tenant`。所有操作在租户上下文事务中执行，兼容 RLS FORCE。

use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// 单个租户的清理明细。
#[derive(Debug, Clone)]
pub struct PurgeTenant {
    /// 租户编码。
    pub code: String,
    /// 删除的用户数。
    pub users: i64,
    /// 删除的文件数。
    pub files: i64,
}

/// 清理结果。
#[derive(Debug, Clone)]
pub struct PurgeOutcome {
    /// 命中的租户编码。
    pub tenants: Vec<String>,
    /// 实际删除的租户数。
    pub purged: u64,
    /// 每个租户的清理明细。
    pub details: Vec<PurgeTenant>,
}

/// 清理超过保留期的已删除租户。
pub async fn purge_deleted_tenants(
    pool: &PgPool,
    retention_days: i64,
    dry_run: bool,
) -> anyhow::Result<PurgeOutcome> {
    let candidates: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, code FROM sys.sys_tenant
         WHERE deleted_at IS NOT NULL
           AND deleted_at < now() - make_interval(days => $1::int)
         ORDER BY deleted_at",
    )
    .bind(retention_days as i32)
    .fetch_all(pool)
    .await?;

    let tables = tenant_tables(pool).await?;
    let edges = fk_edges(pool, &tables).await?;
    let order = delete_order(&tables, &edges);

    let mut purged = 0;
    let mut details = Vec::new();
    for (tenant_id, code) in &candidates {
        if dry_run {
            continue;
        }
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await?;
        let users: i64 =
            sqlx::query_scalar("SELECT count(*)::bigint FROM sys.sys_user WHERE tenant_id = $1")
                .bind(tenant_id)
                .fetch_one(&mut *tx)
                .await?;
        let files: i64 =
            sqlx::query_scalar("SELECT count(*)::bigint FROM sys.sys_file WHERE tenant_id = $1")
                .bind(tenant_id)
                .fetch_one(&mut *tx)
                .await?;
        for table in &order {
            let sql = format!("DELETE FROM {table} WHERE tenant_id = $1");
            sqlx::query(&sql).bind(tenant_id).execute(&mut *tx).await?;
        }
        sqlx::query("DELETE FROM sys.sys_tenant WHERE id = $1")
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        purged += 1;
        details.push(PurgeTenant {
            code: code.clone(),
            users,
            files,
        });
    }

    Ok(PurgeOutcome {
        tenants: candidates.into_iter().map(|(_, code)| code).collect(),
        purged,
        details,
    })
}

async fn tenant_tables(pool: &PgPool) -> anyhow::Result<Vec<String>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT n.nspname, c.relname
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname IN ('sys', 'crm', 'demo')
          AND c.relkind IN ('r', 'p')
          AND NOT c.relispartition
          AND NOT (n.nspname = 'sys' AND c.relname = 'sys_tenant')
          AND EXISTS (
              SELECT 1 FROM information_schema.columns col
              WHERE col.table_schema = n.nspname
                AND col.table_name = c.relname
                AND col.column_name = 'tenant_id'
          )
        ORDER BY n.nspname, c.relname
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(schema, table)| format!("\"{schema}\".\"{table}\""))
        .collect())
}

async fn fk_edges(
    pool: &PgPool,
    tables: &[String],
) -> anyhow::Result<HashMap<String, HashSet<String>>> {
    let known: HashSet<&str> = tables.iter().map(String::as_str).collect();
    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"
        SELECT child_ns.nspname || '.' || child.relname,
               parent_ns.nspname || '.' || parent.relname
        FROM pg_constraint con
        JOIN pg_class child ON child.oid = con.conrelid
        JOIN pg_namespace child_ns ON child_ns.oid = child.relnamespace
        JOIN pg_class parent ON parent.oid = con.confrelid
        JOIN pg_namespace parent_ns ON parent_ns.oid = parent.relnamespace
        WHERE con.contype = 'f'
          AND child_ns.nspname IN ('sys', 'crm', 'demo')
          AND parent_ns.nspname IN ('sys', 'crm', 'demo')
        "#,
    )
    .fetch_all(pool)
    .await?;
    let mut edges: HashMap<String, HashSet<String>> = HashMap::new();
    for (child, parent) in rows {
        let child = format!(
            "\"{}\".\"{}\"",
            child.split('.').next().unwrap(),
            child.split('.').nth(1).unwrap()
        );
        let parent = format!(
            "\"{}\".\"{}\"",
            parent.split('.').next().unwrap(),
            parent.split('.').nth(1).unwrap()
        );
        if child != parent && known.contains(child.as_str()) && known.contains(parent.as_str()) {
            edges.entry(parent).or_default().insert(child);
        }
    }
    Ok(edges)
}

/// 叶子表优先：没有其他表引用它的节点先删。
fn delete_order(tables: &[String], dependents: &HashMap<String, HashSet<String>>) -> Vec<String> {
    let mut remaining: HashSet<String> = tables.iter().cloned().collect();
    let mut order = Vec::with_capacity(tables.len());
    while !remaining.is_empty() {
        let next = remaining
            .iter()
            .find(|t| {
                dependents
                    .get(*t)
                    .map(|set| set.iter().all(|child| !remaining.contains(child)))
                    .unwrap_or(true)
            })
            .cloned()
            .or_else(|| remaining.iter().next().cloned());
        if let Some(table) = next {
            remaining.remove(&table);
            order.push(table);
        }
    }
    order
}
