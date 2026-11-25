//! 审计日志。
//!
//! # 记录什么
//!
//! 只记录**变更类操作**（写接口）与登录事件，查询类默认不记（量太大且价值低）。
//!
//! # 为什么不用中间件自动记录
//!
//! 中间件只能拿到 URL 和状态码，拿不到业务语义（改了哪个用户、改前改后是什么）。
//! 显式调用虽然多写一行，但记录的信息质量高得多，也便于按业务事件检索。
//!
//! # 性能
//!
//! 审计写入是**尽力而为**：失败只打日志，不影响主流程。
//! 真正的强一致审计应走 outbox 与业务同事务（见 `docs/01-后端架构.md` 第 13 节），
//! 当前实现是 M1 的简化版。

use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

pub use crate::audit_maintenance::{drop_old_partitions, ensure_partitions, purge_older_than};

/// 写一条审计记录。
///
/// - `action`：形如 `system.user.create`，与权限点命名区分开（审计是"发生了什么"，权限是"能做什么"）。
/// - `resource_id`：被操作对象的 id（字符串形式，兼容非 uuid 主键）。
/// - `detail`：变更摘要（不要放密码、完整身份证号等敏感信息）。
pub async fn record(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Option<Uuid>,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    detail: Option<Value>,
) {
    record_with_meta(
        pool,
        tenant_id,
        user_id,
        action,
        resource_type,
        resource_id,
        None,
        None,
        detail,
    )
    .await;
}

/// 写入一条租户级告警（用于配额等系统事件）。
pub async fn record_alert(
    pool: &PgPool,
    tenant_id: Uuid,
    action: &str,
    count: i64,
    threshold: i32,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        r#"
        INSERT INTO sys.sys_audit_alert
            (id, tenant_id, action, window_start, window_end, count, threshold)
        VALUES (gen_random_uuid(), $1, $2, date_trunc('minute', now()), now(), $3, $4)
        ON CONFLICT (tenant_id, action, window_start)
        DO UPDATE SET count = sys.sys_audit_alert.count + 1, window_end = now()
        "#,
    )
    .bind(tenant_id)
    .bind(action)
    .bind(count)
    .bind(threshold)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// 以当前登录用户身份写审计（自动带租户、用户、IP、UA）。
pub async fn record_user(
    pool: &PgPool,
    user: &crate::auth::CurrentUser,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    detail: Option<Value>,
) {
    record_with_meta(
        pool,
        user.tenant_id,
        Some(user.user_id),
        action,
        resource_type,
        resource_id,
        user.ip.as_deref(),
        user.user_agent.as_deref(),
        detail,
    )
    .await;
}

/// 写审计记录，并记录来源 IP / User-Agent。
#[allow(clippy::too_many_arguments)]
pub async fn record_with_meta(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Option<Uuid>,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    ip: Option<&str>,
    user_agent: Option<&str>,
    detail: Option<Value>,
) {
    // 审计写入自带租户事务：开启 RLS FORCE 后也能正常工作
    let result = async {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            r#"
            INSERT INTO sys.sys_audit_log
                (id, tenant_id, user_id, action, resource_type, resource_id, result,
                 ip, user_agent, detail)
            VALUES ($1, $2, $3, $4, $5, $6, 'success', $7::text::inet, $8, $9)
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(tenant_id)
        .bind(user_id)
        .bind(action)
        .bind(resource_type)
        .bind(resource_id)
        .bind(ip.map(str::to_owned))
        .bind(user_agent)
        .bind(detail)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok::<(), sqlx::Error>(())
    }
    .await;

    if let Err(e) = result {
        // 审计失败不能影响业务，但必须留下痕迹便于排查
        tracing::error!(error = %e, action, "审计日志写入失败");
    }
}

/// 读取一行的 JSON 快照（用于审计 before/after）。
///
/// `table` 只能是代码内写死的表名，不接受用户输入。
pub async fn snapshot(
    conn: &mut sqlx::PgConnection,
    table: &str,
    id: Uuid,
    tenant_id: Uuid,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    let sql = format!("SELECT to_jsonb(t) FROM {table} t WHERE t.id = $1 AND t.tenant_id = $2");
    sqlx::query_scalar(&sql)
        .bind(id)
        .bind(tenant_id)
        .fetch_optional(conn)
        .await
}

pub use crate::audit_notify::notify_pending;
pub use crate::audit_subscription_dispatch::dispatch_subscriptions;

/// 扫描审计日志并生成异常告警。
///
/// 规则：最近 `window_minutes` 分钟内，同一 action 次数 >= `threshold`。
/// 同一窗口重复扫描时更新 count，不会重复插入。
pub async fn detect_alerts(pool: &PgPool) -> anyhow::Result<u64> {
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
        let mut rules: Vec<(String, i32, i32, i32)> = sqlx::query_as(
            "SELECT action_pattern, window_minutes, threshold, silence_minutes
             FROM sys.sys_audit_alert_rule
             WHERE tenant_id = $1 AND enabled
             ORDER BY length(action_pattern) DESC",
        )
        .bind(tenant)
        .fetch_all(&mut *tx)
        .await?;
        if rules.is_empty() {
            rules.push(("*".to_string(), 5, 50, 30));
        }

        let mut changed = 0u64;
        for (pattern, window_minutes, threshold, silence_minutes) in rules {
            let like_pattern = if pattern == "*" {
                "%".to_string()
            } else {
                pattern
            };
            let result = sqlx::query(
                r#"
            INSERT INTO sys.sys_audit_alert
                (id, tenant_id, action, window_start, window_end, count, threshold)
            SELECT gen_random_uuid(),
                   $1,
                   l.action,
                   date_trunc('hour', now())
                     + (floor(extract(minute from now()) / $3) * $3) * interval '1 minute',
                   now(),
                   count(*),
                   $2
            FROM sys.sys_audit_log l
            WHERE l.tenant_id = $1
              AND l.action LIKE $4
              AND l.created_at >= now() - make_interval(mins => $3::int)
            GROUP BY l.action
            HAVING count(*) >= $2
               AND NOT EXISTS (
                   SELECT 1 FROM sys.sys_audit_alert a
                   WHERE a.tenant_id = $1
                     AND a.action = l.action
                     AND a.created_at >= now() - make_interval(mins => $5)
               )
            ON CONFLICT (tenant_id, action, window_start)
            DO UPDATE SET count = EXCLUDED.count, window_end = EXCLUDED.window_end
            "#,
            )
            .bind(tenant)
            .bind(threshold)
            .bind(window_minutes)
            .bind(&like_pattern)
            .bind(silence_minutes)
            .execute(&mut *tx)
            .await?;
            changed += result.rows_affected();
        }
        tx.commit().await?;
        total += changed;
    }
    Ok(total)
}
