//! 审计日志维护：保留期清理与按月分区管理。
//!
//! 边界：审计写入/告警在 `audit`；本模块只做数据生命周期维护。
//! 关键约束：分区操作使用 advisory lock，避免多实例并发建/删同一分区。

use sqlx::PgPool;
use uuid::Uuid;

/// 清理超过保留期的审计日志。
///
/// 审计表是 RLS FORCE，必须逐租户设置 `app.tenant_id` 后删除。
/// 返回删除行数；后台任务每日执行一次。
pub async fn purge_older_than(pool: &PgPool, days: u64) -> anyhow::Result<u64> {
    let tenants: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM sys.sys_tenant")
        .fetch_all(pool)
        .await?;
    let mut total = 0u64;
    for tenant in tenants {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        let deleted = sqlx::query(
            r#"
            DELETE FROM sys.sys_audit_log
            WHERE tenant_id = $1
              AND created_at < now() - make_interval(days => $2::int)
            "#,
        )
        .bind(tenant)
        .bind(days as i32)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        total += deleted;
    }
    Ok(total)
}

/// 确保未来 N 个月的审计分区存在（幂等）。
pub async fn ensure_partitions(pool: &PgPool, months_ahead: u32) -> anyhow::Result<()> {
    use chrono::{Datelike, Months, Utc};
    let now = Utc::now();
    let base = now.with_day(1).unwrap_or(now);
    for i in 0..=months_ahead {
        let start = base.checked_add_months(Months::new(i)).unwrap_or(base);
        let end = start.checked_add_months(Months::new(1)).unwrap_or(start);
        let table = format!("sys_audit_log_{}", start.format("%Y%m"));
        let sql = format!(
            "CREATE TABLE IF NOT EXISTS sys.{table} PARTITION OF sys.sys_audit_log              FOR VALUES FROM ('{}') TO ('{}')",
            start.format("%Y-%m-%d"),
            end.format("%Y-%m-%d")
        );
        sqlx::query(&sql).execute(pool).await?;
    }
    Ok(())
}

/// 删除早于保留期的审计分区，返回删除的分区数。
pub async fn drop_old_partitions(pool: &PgPool, days: u64) -> anyhow::Result<u64> {
    use chrono::{Duration, Utc};
    let cutoff = (Utc::now() - Duration::days(days as i64))
        .format("%Y%m")
        .to_string();
    let names: Vec<String> = sqlx::query_scalar(
        r#"SELECT relname FROM pg_class WHERE relname ~ '^sys_audit_log_[0-9]{6}$'"#,
    )
    .fetch_all(pool)
    .await?;

    let mut dropped = 0u64;
    for name in names {
        let ym = &name["sys_audit_log_".len()..];
        if ym < cutoff.as_str() {
            sqlx::query(&format!("DROP TABLE IF EXISTS sys.{name}"))
                .execute(pool)
                .await?;
            dropped += 1;
        }
    }
    Ok(dropped)
}
