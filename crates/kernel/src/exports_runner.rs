//! 导出任务的后台执行：运行、失败标记与陈旧任务回收。
//!
//! 边界：导出任务 HTTP 接口在 `exports`；本模块只做后台执行。
//! 关键约束：逐租户设置 RLS 上下文；失败任务写 failed 并记录原因。

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::exports::{export_audit, export_users, find_export, ExportContext};
use crate::state::AppState;
use crate::tenant::TenantTx;

/// 后台导出任务的参数（避免函数参数过多）。
pub(crate) struct ExportJob {
    pub(crate) task_id: Uuid,
    pub(crate) tenant_id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) kind: String,
    pub(crate) keyword: Option<String>,
    pub(crate) resource_type: Option<String>,
    pub(crate) ip: Option<String>,
    pub(crate) user_agent: Option<String>,
    pub(crate) from: Option<DateTime<Utc>>,
    pub(crate) to: Option<DateTime<Utc>>,
    pub(crate) data_scope: String,
    pub(crate) dept_id: Option<Uuid>,
}

/// 后台执行导出。
///
/// 分三段、两个事务：
/// 1. 事务 A：置 running → 生成字节（查询 + xlsx）；
/// 2. 事务外：落盘（外部 IO 不占用连接与事务）；
/// 3. 事务 B：写 success。
pub(crate) async fn run_export(state: &AppState, job: ExportJob) -> anyhow::Result<()> {
    let ExportJob {
        task_id,
        tenant_id,
        user_id,
        kind,
        keyword,
        resource_type,
        ip,
        user_agent,
        from,
        to,
        data_scope,
        dept_id,
    } = job;
    let ctx = ExportContext {
        user_id,
        tenant_id,
        data_scope,
        dept_id,
    };

    let mut tx = TenantTx::begin(&state.db, tenant_id, user_id).await?;

    sqlx::query(
        "UPDATE sys.sys_export_task SET status = 'running', started_at = now() WHERE id = $1",
    )
    .bind(task_id)
    .execute(&mut *tx)
    .await?;

    let (bytes, file_name, rows) = if let Some(reg) = find_export(&kind) {
        let file = (reg.run)(&mut tx, &ctx).await?;
        (file.bytes, file.file_name, file.rows)
    } else {
        match kind.as_str() {
            "system.audit" => {
                export_audit(
                    &mut tx,
                    keyword.as_deref(),
                    resource_type.as_deref(),
                    ip.as_deref(),
                    user_agent.as_deref(),
                    from,
                    to,
                )
                .await?
            }
            "system.users" => export_users(&mut tx, &ctx).await?,
            other => anyhow::bail!("未知导出类型: {other}"),
        }
    };

    // 先释放连接：落盘是外部 IO，占着事务等磁盘会把连接池拖死。
    tx.commit().await?;

    let key = format!("{tenant_id}/exports/{task_id}.xlsx");
    state.storage.save(&key, &bytes).await?;

    let mut tx = TenantTx::begin(&state.db, tenant_id, user_id).await?;
    sqlx::query(
        r#"
        UPDATE sys.sys_export_task
        SET status = 'success', file_key = $2, file_name = $3, total_rows = $4, finished_at = now()
        WHERE id = $1
        "#,
    )
    .bind(task_id)
    .bind(&key)
    .bind(&file_name)
    .bind(rows as i64)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

/// 标记失败（用独立事务，避免原事务已回滚）。
pub(crate) async fn mark_failed(
    state: &AppState,
    task_id: Uuid,
    tenant_id: Uuid,
    user_id: Uuid,
    error: &str,
) {
    let Ok(mut tx) = TenantTx::begin(&state.db, tenant_id, user_id).await else {
        return;
    };
    let _ = sqlx::query(
        "UPDATE sys.sys_export_task SET status = 'failed', error = $2, finished_at = now() WHERE id = $1",
    )
    .bind(task_id)
    .bind(error)
    .execute(&mut *tx)
    .await;
    let _ = tx.commit().await;
}

/// 启动时收尾「超时未完成」的导出任务，并清理过期的历史任务。
///
/// 只回收超过 15 分钟的 pending/running 任务：滚动发布时新实例启动，
/// 若一刀切把所有任务置失败，会误杀仍在其他实例上运行的任务。
/// 失败即终态，用户可重新发起；已完成的记录保留 30 天用于追溯。
pub async fn recover_stale(pool: &sqlx::PgPool) {
    // sys_export_task 已 FORCE RLS：无法跨租户裸更新，
    // 必须逐租户设置 app.tenant_id 后再收尾，否则 UPDATE 会静默匹配 0 行。
    let tenants: Vec<Uuid> = match sqlx::query_scalar("SELECT id FROM sys.sys_tenant")
        .fetch_all(pool)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "读取租户列表失败，跳过中断任务恢复");
            return;
        }
    };

    for tenant_id in tenants {
        let result = async {
            let mut tx = pool.begin().await?;
            sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
                .bind(tenant_id.to_string())
                .execute(&mut *tx)
                .await?;
            let affected = sqlx::query(
                "UPDATE sys.sys_export_task
                 SET status = 'failed', error = '服务重启，任务中断', finished_at = now()
                 WHERE tenant_id = $1
                   AND status IN ('pending', 'running')
                   AND COALESCE(started_at, created_at) < now() - interval '15 minutes'",
            )
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();

            let purged = sqlx::query(
                "DELETE FROM sys.sys_export_task
                 WHERE tenant_id = $1
                   AND status IN ('success', 'failed')
                   AND finished_at < now() - interval '30 days'",
            )
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();

            tx.commit().await?;
            Ok::<(u64, u64), sqlx::Error>((affected, purged))
        }
        .await;

        match result {
            Ok((stale, purged)) if stale > 0 || purged > 0 => {
                tracing::info!(
                    tenant = %tenant_id,
                    stale,
                    purged,
                    "导出任务维护完成（超时置失败 / 过期清理）"
                );
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, tenant = %tenant_id, "恢复中断的导出任务失败"),
        }
    }
}
