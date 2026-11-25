//! 导出任务维护集成测试（真库）。
//!
//! 验证 `recover_stale` 的两条关键语义：
//! - 只回收「超过 15 分钟」的 pending/running，刚创建的任务不能被误杀（滚动发布场景）；
//! - 30 天前已完成的任务被清理。
//!
//! 运行：
//! ```powershell
//! $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin_export"
//! cargo test -p server --test export_maintenance -- --ignored
//! ```

use sqlx::PgPool;
use uuid::Uuid;

use plugin_crm as _;
use plugin_demo as _;

/// 插入一条测试任务，返回它的 id。
///
/// 返回 id 是为了**按行断言**：租户里还有历史任务（开发库反复运行、
/// 其他测试留下的数据），按 status 计数会被污染，测出来的失败与代码无关。
async fn seed_task(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    status: &str,
    created_offset_minutes: i32,
    finished_offset_days: Option<i32>,
) -> Uuid {
    let id = Uuid::now_v7();
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "INSERT INTO sys.sys_export_task
           (id, tenant_id, user_id, kind, status, created_at, started_at, finished_at)
         VALUES ($1, $2, $3, 'system.audit', $4,
                 now() - make_interval(mins => $5),
                 now() - make_interval(mins => $5),
                 CASE WHEN $6::int IS NULL THEN NULL
                      ELSE now() - make_interval(days => $6) END)",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(user_id)
    .bind(status)
    .bind(created_offset_minutes)
    .bind(finished_offset_days)
    .execute(&mut *tx)
    .await
    .expect("insert task");
    tx.commit().await.expect("commit");
    id
}

/// 读取指定任务当前的状态；行已被删除时返回 `None`。
async fn task_status(pool: &PgPool, tenant_id: Uuid, task_id: Uuid) -> Option<String> {
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let status = sqlx::query_scalar(
        "SELECT status FROM sys.sys_export_task WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id)
    .bind(task_id)
    .fetch_optional(&mut *tx)
    .await
    .expect("count");
    tx.commit().await.expect("commit");
    status
}

#[tokio::test]
#[ignore = "需要真实 PostgreSQL（DATABASE_URL）"]
async fn recover_stale_only_reclaims_aged_tasks() {
    std::env::set_var(
        "DATABASE_URL",
        std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL"),
    );
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");
    let pool = app.state.db.clone();

    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.tenant_id', (SELECT id::text FROM sys.sys_tenant WHERE code = 'default'), true)")
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    let tenant_id: Uuid =
        sqlx::query_scalar("SELECT id FROM sys.sys_tenant WHERE code = 'default'")
            .fetch_one(&mut *tx)
            .await
            .expect("tenant");
    let user_id: Uuid = sqlx::query_scalar("SELECT id FROM sys.sys_user WHERE username = 'admin'")
        .fetch_one(&mut *tx)
        .await
        .expect("admin");
    tx.commit().await.expect("commit");

    // 超时任务（应置失败）、刚建任务（应保留）、过期完成任务（应删除）
    let stale_id = seed_task(&pool, tenant_id, user_id, "pending", 30, None).await;
    let fresh_id = seed_task(&pool, tenant_id, user_id, "pending", 0, None).await;
    let expired_id = seed_task(&pool, tenant_id, user_id, "success", 60 * 24 * 50, Some(40)).await;

    kernel::exports::recover_stale(&pool).await;

    // 按「本测试插入的行」断言：租户里可能还有历史数据，
    // 按 status 聚合计数会把别处的行也算进来。
    assert_eq!(
        task_status(&pool, tenant_id, stale_id).await.as_deref(),
        Some("failed"),
        "超过 15 分钟的 pending 任务应被标记失败"
    );
    assert_eq!(
        task_status(&pool, tenant_id, fresh_id).await.as_deref(),
        Some("pending"),
        "刚创建的任务不应被回收"
    );
    assert_eq!(
        task_status(&pool, tenant_id, expired_id).await,
        None,
        "40 天前完成的任务应被清理"
    );
}
