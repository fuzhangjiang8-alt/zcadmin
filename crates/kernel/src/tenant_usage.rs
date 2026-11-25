//! 租户用量日快照。
//!
//! 每天记录一次用户/文件/存储与 CRM 业务量，供运营趋势查看。

use chrono::NaiveDate;
use sqlx::PgPool;
use uuid::Uuid;

/// 单日用量。
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsagePoint {
    /// 日期。
    pub snapshot_date: NaiveDate,
    /// 用户数。
    pub user_count: i64,
    /// 文件数。
    pub file_count: i64,
    /// 存储字节。
    pub storage_used: i64,
    /// 客户数。
    pub customer_count: i64,
    /// 合同数。
    pub contract_count: i64,
}

/// 为所有活跃租户记录当日快照。
pub async fn capture_usage(pool: &PgPool) -> anyhow::Result<u64> {
    let tenants: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM sys.sys_tenant WHERE deleted_at IS NULL")
            .fetch_all(pool)
            .await?;
    let has_crm: bool = sqlx::query_scalar("SELECT to_regclass('crm.customer') IS NOT NULL")
        .fetch_one(pool)
        .await?;
    let mut captured = 0;
    for tenant in tenants {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        let users: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM sys.sys_user WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await?;
        let files: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM sys.sys_file WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await?;
        let storage: i64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(size_bytes), 0)::bigint FROM sys.sys_file
             WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await?;
        let previous: Option<(i64, i64)> = sqlx::query_as(
            "SELECT user_count, storage_used FROM sys.sys_tenant_usage_snapshot
             WHERE tenant_id = $1 AND snapshot_date < current_date
             ORDER BY snapshot_date DESC LIMIT 1",
        )
        .bind(tenant)
        .fetch_optional(&mut *tx)
        .await?;
        let (customers, contracts) = if has_crm {
            let c: i64 = sqlx::query_scalar(
                "SELECT count(*)::bigint FROM crm.customer WHERE tenant_id = $1 AND deleted_at IS NULL",
            )
            .bind(tenant)
            .fetch_one(&mut *tx)
            .await?;
            let k: i64 = sqlx::query_scalar(
                "SELECT count(*)::bigint FROM crm.contract WHERE tenant_id = $1 AND deleted_at IS NULL",
            )
            .bind(tenant)
            .fetch_one(&mut *tx)
            .await?;
            (c, k)
        } else {
            (0, 0)
        };
        sqlx::query(
            r#"
            INSERT INTO sys.sys_tenant_usage_snapshot
                (id, tenant_id, snapshot_date, user_count, file_count, storage_used,
                 customer_count, contract_count)
            VALUES (gen_random_uuid(), $1, current_date, $2, $3, $4, $5, $6)
            ON CONFLICT (tenant_id, snapshot_date) DO UPDATE
            SET user_count = EXCLUDED.user_count,
                file_count = EXCLUDED.file_count,
                storage_used = EXCLUDED.storage_used,
                customer_count = EXCLUDED.customer_count,
                contract_count = EXCLUDED.contract_count,
                created_at = now()
            "#,
        )
        .bind(tenant)
        .bind(users)
        .bind(files)
        .bind(storage)
        .bind(customers)
        .bind(contracts)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        if let Some((prev_users, prev_storage)) = previous {
            if is_spike(users, prev_users, 3) {
                let _ = crate::audit::record_alert(
                    pool,
                    tenant,
                    "system.tenant.usage.user_spike",
                    users,
                    prev_users.min(i32::MAX as i64) as i32,
                )
                .await;
            }
            if is_spike(storage, prev_storage, 10 * 1024 * 1024) {
                let _ = crate::audit::record_alert(
                    pool,
                    tenant,
                    "system.tenant.usage.storage_spike",
                    storage,
                    prev_storage.min(i32::MAX as i64) as i32,
                )
                .await;
            }
        }
        captured += 1;
    }
    Ok(captured)
}

/// 查询某租户最近 N 天用量。
pub async fn usage_history(
    conn: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    days: i64,
) -> anyhow::Result<Vec<UsagePoint>> {
    let rows = sqlx::query_as(
        r#"
        SELECT snapshot_date, user_count, file_count, storage_used,
               customer_count, contract_count
        FROM sys.sys_tenant_usage_snapshot
        WHERE tenant_id = $1
          AND snapshot_date >= current_date - $2::int
        ORDER BY snapshot_date
        "#,
    )
    .bind(tenant_id)
    .bind(days as i32)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows)
}

/// 是否较上次快照出现显著增长：绝对增量达到阈值且增幅超过 50%。
fn is_spike(current: i64, previous: i64, min_delta: i64) -> bool {
    current > previous
        && current - previous >= min_delta
        && current.saturating_mul(2) > previous.saturating_mul(3)
}

/// 生成指定月份账单（幂等，重复生成覆盖草稿）。
pub async fn generate_bills(pool: &PgPool, month: &str) -> anyhow::Result<u64> {
    let bill_month = chrono::NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("月份格式必须为 YYYY-MM"))?;
    let tenants: Vec<(Uuid, String, i32, i32)> = sqlx::query_as(
        "SELECT id, currency, price_per_user_month_cents, price_per_gb_month_cents
         FROM sys.sys_tenant WHERE deleted_at IS NULL ORDER BY code",
    )
    .fetch_all(pool)
    .await?;
    let mut generated = 0;
    for (id, currency, price_user, price_gb) in tenants {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;
        let (avg_users, avg_storage, snaps): (f64, f64, i64) = sqlx::query_as(
            "SELECT COALESCE(AVG(user_count), 0)::float8,
                    COALESCE(AVG(storage_used), 0)::float8,
                    count(*)::bigint
             FROM sys.sys_tenant_usage_snapshot
             WHERE tenant_id = $1
               AND snapshot_date >= $2
               AND snapshot_date < $2 + interval '1 month'",
        )
        .bind(id)
        .bind(bill_month)
        .fetch_one(&mut *tx)
        .await?;
        let (avg_users, avg_storage) = if snaps == 0 {
            let users: i64 = sqlx::query_scalar(
                "SELECT count(*)::bigint FROM sys.sys_user WHERE tenant_id = $1 AND deleted_at IS NULL",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            let storage: i64 = sqlx::query_scalar(
                "SELECT COALESCE(sum(size_bytes), 0)::bigint FROM sys.sys_file
                 WHERE tenant_id = $1 AND deleted_at IS NULL",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            (users as f64, storage as f64)
        } else {
            (avg_users, avg_storage)
        };
        // 保留事务到账单写入之后再提交：账单表已 FORCE RLS，
        // 必须先有 app.tenant_id 上下文才能写入。
        let storage_gb = avg_storage / 1024.0 / 1024.0 / 1024.0;
        let user_amount = (avg_users * f64::from(price_user)).ceil() as i64;
        let storage_amount = (storage_gb * f64::from(price_gb)).ceil() as i64;
        let result = sqlx::query(
            r#"
            INSERT INTO sys.sys_tenant_bill
                (id, tenant_id, bill_month, currency, avg_users, avg_storage_bytes,
                 user_amount_cents, storage_amount_cents, total_cents, status)
            VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7, $8, 'draft')
            ON CONFLICT (tenant_id, bill_month) DO UPDATE
            SET currency = EXCLUDED.currency,
                avg_users = EXCLUDED.avg_users,
                avg_storage_bytes = EXCLUDED.avg_storage_bytes,
                user_amount_cents = EXCLUDED.user_amount_cents,
                storage_amount_cents = EXCLUDED.storage_amount_cents,
                total_cents = EXCLUDED.total_cents,
                updated_at = now()
            WHERE sys_tenant_bill.status = 'draft'
            "#,
        )
        .bind(id)
        .bind(bill_month)
        .bind(currency)
        .bind(avg_users)
        .bind(avg_storage as i64)
        .bind(user_amount)
        .bind(storage_amount)
        .bind(user_amount + storage_amount)
        .execute(&mut *tx)
        .await?;
        // 只有真正写入（新插入或仍是草稿）才计数；已出账/已收款的账单保持原值。
        generated += result.rows_affected();
        tx.commit().await?;
    }
    Ok(generated)
}

#[cfg(test)]
mod tests {
    use super::is_spike;

    #[test]
    fn spike_requires_delta_and_ratio() {
        assert!(is_spike(10, 4, 3));
        assert!(!is_spike(5, 4, 3));
        assert!(!is_spike(11, 10, 3));
        assert!(!is_spike(10, 10, 3));
    }
}
