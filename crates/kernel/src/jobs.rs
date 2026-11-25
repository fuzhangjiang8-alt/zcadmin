//! 后台周期任务与多实例选主锁。
//!
//! # 多实例并发安全
//!
//! 多副本部署时，若每个实例同时运行审计告警检测、月度出账、逾期扫描或租户清理，
//! 会产生重复告警或锁竞争。本模块通过 PostgreSQL 会话级咨询锁
//! `pg_try_advisory_lock(lock_id)` 实现非阻塞选主：同一周期内只有抢到锁的实例执行，
//! 其余实例立即跳过；任务完成后显式释放锁（连接意外断开时由 PG 自动释放）。

use crate::config::Config;
use crate::state::AppState;
use std::future::Future;
use std::time::Duration;

const LOCK_AUDIT_ALERT: i64 = 0x7A63_0001;
const LOCK_AUDIT_PARTITION: i64 = 0x7A63_0002;
const LOCK_TENANT_PURGE: i64 = 0x7A63_0003;
const LOCK_TENANT_USAGE: i64 = 0x7A63_0004;
const LOCK_TENANT_BILL: i64 = 0x7A63_0005;
const LOCK_TENANT_OVERDUE: i64 = 0x7A63_0006;
const LOCK_COLLECTION_REMIND: i64 = 0x7A63_0007;

/// 尝试获取 PostgreSQL 会话级咨询锁并在持锁期间执行任务；未抢到锁则静默跳过。
async fn try_with_leader_lock<F, Fut>(pool: &sqlx::PgPool, lock_id: i64, task: F)
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    let Ok(mut conn) = pool.acquire().await else {
        return;
    };
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(lock_id)
        .fetch_one(&mut *conn)
        .await
        .unwrap_or(false);
    if !acquired {
        return;
    }
    task().await;
    let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(lock_id)
        .execute(&mut *conn)
        .await;
}

/// 启动全部后台周期任务（会话清理、审计告警与分区维护、租户计费与清理）。
pub fn spawn(state: AppState, config: &Config) {
    let session_state = state.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(600));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match session_state.sessions.purge_expired().await {
                Ok(removed) if removed > 0 => tracing::info!(removed, "已清理过期会话"),
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "清理过期会话失败"),
            }
        }
    });

    let alert_pool = state.db.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(300));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let pool = alert_pool.clone();
            try_with_leader_lock(&alert_pool, LOCK_AUDIT_ALERT, || async move {
                match crate::audit::detect_alerts(&pool).await {
                    Ok(n) if n > 0 => tracing::warn!(alerts = n, "检测到审计异常"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "审计告警扫描失败"),
                }
                match crate::audit::notify_pending(&pool).await {
                    Ok(n) if n > 0 => tracing::info!(notified = n, "审计告警已推送"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "审计告警推送失败"),
                }
                match crate::audit::dispatch_subscriptions(&pool).await {
                    Ok(n) if n > 0 => tracing::info!(sent = n, "订阅告警已推送"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "订阅告警推送失败"),
                }
            })
            .await;
        }
    });

    spawn_daily_maintenance(&state, config);
    crate::telemetry::spawn_worker(state);
    tracing::info!("后台任务已启动（含 PostgreSQL advisory lock 多实例防重与异步探针）");
}

/// 启动 24 小时周期的分区、租户与计费维护任务。
fn spawn_daily_maintenance(state: &AppState, config: &Config) {
    let day = Duration::from_secs(24 * 3600);

    let retention_pool = state.db.clone();
    let retention_days = config.audit.retention_days;
    tokio::spawn(async move {
        loop {
            let pool = retention_pool.clone();
            try_with_leader_lock(&retention_pool, LOCK_AUDIT_PARTITION, || async move {
                if let Err(e) = crate::audit::ensure_partitions(&pool, 2).await {
                    tracing::warn!(error = %e, "创建审计分区失败");
                }
                match crate::audit::drop_old_partitions(&pool, retention_days).await {
                    Ok(n) if n > 0 => {
                        tracing::info!(dropped = n, days = retention_days, "已删除过期审计分区")
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "删除过期审计分区失败"),
                }
            })
            .await;
            tokio::time::sleep(day).await;
        }
    });

    let purge_pool = state.db.clone();
    let tenant_retention = config.tenant.retention_days;
    tokio::spawn(async move {
        loop {
            let pool = purge_pool.clone();
            try_with_leader_lock(&purge_pool, LOCK_TENANT_PURGE, || async move {
                match crate::tenant_purge::purge_deleted_tenants(
                    &pool,
                    tenant_retention as i64,
                    false,
                )
                .await
                {
                    Ok(outcome) if !outcome.tenants.is_empty() => {
                        tracing::info!(
                            purged = outcome.purged,
                            days = tenant_retention,
                            "已清理过期租户"
                        )
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "清理过期租户失败"),
                }
            })
            .await;
            tokio::time::sleep(day).await;
        }
    });

    let usage_pool = state.db.clone();
    tokio::spawn(async move {
        loop {
            let pool = usage_pool.clone();
            try_with_leader_lock(&usage_pool, LOCK_TENANT_USAGE, || async move {
                match crate::tenant_usage::capture_usage(&pool).await {
                    Ok(n) => tracing::info!(tenants = n, "已采集租户用量"),
                    Err(e) => tracing::warn!(error = %e, "采集租户用量失败"),
                }
            })
            .await;
            tokio::time::sleep(day).await;
        }
    });

    let bill_pool = state.db.clone();
    tokio::spawn(async move {
        loop {
            let now = chrono::Utc::now();
            if now.format("%d").to_string() == "01" {
                let prev = (now - chrono::Duration::days(1))
                    .format("%Y-%m")
                    .to_string();
                let pool = bill_pool.clone();
                try_with_leader_lock(&bill_pool, LOCK_TENANT_BILL, || async move {
                    match crate::tenant_usage::generate_bills(&pool, &prev).await {
                        Ok(n) => tracing::info!(month = %prev, bills = n, "已生成月度账单"),
                        Err(e) => tracing::warn!(error = %e, "生成月度账单失败"),
                    }
                })
                .await;
            }
            tokio::time::sleep(day).await;
        }
    });

    let overdue_pool = state.db.clone();
    let overdue_days = config.tenant.bill_overdue_days as i64;
    tokio::spawn(async move {
        loop {
            let pool = overdue_pool.clone();
            try_with_leader_lock(&overdue_pool, LOCK_TENANT_OVERDUE, || async move {
                match crate::system::scan_overdue_bills_internal(&pool, overdue_days).await {
                    Ok(n) if n > 0 => tracing::warn!(bills = n, "已产生逾期账单告警"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "逾期账单扫描失败"),
                }
            })
            .await;
            tokio::time::sleep(day).await;
        }
    });

    let reminder_pool = state.db.clone();
    tokio::spawn(async move {
        loop {
            let pool = reminder_pool.clone();
            try_with_leader_lock(&reminder_pool, LOCK_COLLECTION_REMIND, || async move {
                match crate::system::scan_collection_reminders(&pool, 3).await {
                    Ok(n) if n > 0 => tracing::warn!(records = n, "已产生催收承诺提醒"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "催收提醒扫描失败"),
                }
            })
            .await;
            tokio::time::sleep(day).await;
        }
    });
}
