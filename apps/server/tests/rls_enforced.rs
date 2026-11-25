//! RLS 强制生效验证（需要非超级用户的数据库角色）。
//!
//! # 为什么单独一个文件
//!
//! 其他集成测试用 `DATABASE_URL` 连接，本地/CI 常常是**超级用户**——
//! 超级用户永远绕过 RLS，所以那些测试即使全绿，也证明不了租户隔离。
//! 本文件要求一个 `NOSUPERUSER NOBYPASSRLS` 的角色，专门验证：
//!
//! 1. 没有 `app.tenant_id` 上下文时，受保护表返回 0 行；
//! 2. 设置 `app.tenant_id` 后只能看到该租户的行；
//! 3. 分区父表（`sys_audit_log`）的 FORCE 对子表同样生效。
//!
//! # 运行前提
//!
//! 需要 `RLS_TEST_DATABASE_URL` 指向一个**非超级用户**连接串，
//! 该角色对 `sys` schema 有 `USAGE` + `SELECT`。CI 里由 integration job 创建：
//!
//! ```sql
//! CREATE ROLE rls_probe NOLOGIN NOSUPERUSER NOBYPASSRLS;
//! GRANT USAGE ON SCHEMA sys TO rls_probe;
//! GRANT SELECT ON ALL TABLES IN SCHEMA sys TO rls_probe;
//! ```
//!
//! ```powershell
//! $env:RLS_TEST_DATABASE_URL="postgres://rls_probe@127.0.0.1:5432/zcadmin"
//! cargo test -p server --test rls_enforced -- --ignored --nocapture
//! ```

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// 建立探测连接池，并断言该角色**确实不能**绕过 RLS。
///
/// 如果误用超级用户，测试必须直接失败而不是"假通过"——
/// 那正是本项目历史上踩过的坑（CI 用 zcadmin 超级用户跑 RLS 测试）。
async fn probe_pool() -> PgPool {
    let url = std::env::var("RLS_TEST_DATABASE_URL")
        .expect("必须设置 RLS_TEST_DATABASE_URL（非超级用户连接串）");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("连接探测库失败");

    let row = sqlx::query(
        "SELECT current_user AS who,
                (SELECT rolsuper FROM pg_roles WHERE rolname = current_user) AS is_super,
                (SELECT rolbypassrls FROM pg_roles WHERE rolname = current_user) AS bypass",
    )
    .fetch_one(&pool)
    .await
    .expect("读取角色属性失败");

    let who: String = row.get("who");
    let is_super: bool = row.get("is_super");
    let bypass: bool = row.get("bypass");
    assert!(
        !is_super && !bypass,
        "RLS_TEST_DATABASE_URL 指向的角色 {who} 是超级用户或 BYPASSRLS；\
         用它测不出任何隔离，请改用 NOSUPERUSER NOBYPASSRLS 角色"
    );
    pool
}

/// 取一个真实租户 id，用于设置 `app.tenant_id`。
async fn any_tenant(pool: &PgPool) -> Uuid {
    sqlx::query_scalar("SELECT id FROM sys.sys_tenant ORDER BY created_at LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("库中必须至少有一个租户（先跑一次应用启动以执行 seed）")
}

/// 建立「表 owner」连接池（`RLS_OWNER_DATABASE_URL`）。
///
/// # 为什么需要这一条
///
/// PostgreSQL 的 RLS 分两级：
/// - `ENABLE` 只约束**非 owner** 的普通角色；
/// - `FORCE` 才约束**表 owner**。
///
/// 只用普通角色（`rls_probe`）测，即使某张表忘了 FORCE 也会"通过"——
/// 这类测试是假安全的。生产环境的应用连接往往就是建表角色（本仓库
/// `docker-compose.yml` 里 `zcadmin_app` 同时是 database owner 和建表角色），
/// 所以**必须**用 owner 身份再验一遍。
///
/// 该角色必须是 `NOSUPERUSER NOBYPASSRLS`，但**拥有** `sys` schema 下的表。
async fn owner_pool() -> PgPool {
    let url = std::env::var("RLS_OWNER_DATABASE_URL")
        .expect("必须设置 RLS_OWNER_DATABASE_URL（表 owner 连接串，用于验证 FORCE 是否生效）");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("连接 owner 库失败");

    let row = sqlx::query(
        "SELECT current_user AS who,
                (SELECT rolsuper FROM pg_roles WHERE rolname = current_user) AS is_super,
                (SELECT rolbypassrls FROM pg_roles WHERE rolname = current_user) AS bypass",
    )
    .fetch_one(&pool)
    .await
    .expect("读取角色属性失败");

    let who: String = row.get("who");
    let is_super: bool = row.get("is_super");
    let bypass: bool = row.get("bypass");
    assert!(
        !is_super && !bypass,
        "RLS_OWNER_DATABASE_URL 指向的角色 {who} 是超级用户或 BYPASSRLS；\
         超级用户永远绕过 RLS，连 FORCE 也拦不住，测不出任何东西"
    );

    // 确认它**确实是**表 owner，否则这条测试退化成和 probe 一样的普通角色测试。
    let is_owner: bool = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM pg_tables
             WHERE schemaname = 'sys' AND tablename = 'sys_user' AND tableowner = current_user
         )",
    )
    .fetch_one(&pool)
    .await
    .expect("读取表 owner 失败");
    assert!(
        is_owner,
        "角色 {who} 不是 sys.sys_user 的表 owner；只有 owner 才能验证 FORCE 是否真的生效"
    );

    pool
}

/// 没有租户上下文时，受保护表必须返回 0 行。
///
/// 这是最关键的一条：任何"忘记 set_config"的查询路径，
/// 在 FORCE RLS 下都应该看不到数据，而不是看到全部数据。
#[tokio::test]
#[ignore = "需要非超级用户角色（RLS_TEST_DATABASE_URL）"]
async fn protected_tables_are_empty_without_tenant_context() {
    let pool = probe_pool().await;

    for table in [
        "sys.sys_user",
        "sys.sys_role",
        "sys.sys_menu",
        "sys.sys_dept",
        "sys.sys_config",
        "sys.sys_file",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| panic!("查询 {table} 失败（是否给探测角色授了 SELECT？）: {e}"));
        assert_eq!(
            count, 0,
            "{table} 在无 app.tenant_id 上下文时返回了 {count} 行——RLS 未生效或未 FORCE"
        );
    }
}

/// 设置租户上下文后，只能看到该租户的行。
#[tokio::test]
#[ignore = "需要非超级用户角色（RLS_TEST_DATABASE_URL）"]
async fn tenant_context_scopes_rows_to_one_tenant() {
    let pool = probe_pool().await;
    let tenant = any_tenant(&pool).await;

    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM sys.sys_user")
        .fetch_one(&mut *tx)
        .await
        .expect("count users");
    assert!(rows > 0, "设置上下文后应能看到本租户用户");

    // 跨租户断言：所有可见行的 tenant_id 必须都是当前租户
    let wrong: i64 = sqlx::query_scalar("SELECT count(*) FROM sys.sys_user WHERE tenant_id <> $1")
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await
        .expect("count foreign rows");
    assert_eq!(wrong, 0, "看到了其他租户的行——RLS 策略失效");

    tx.commit().await.expect("commit");
}

/// 审计表按月分区：父表 FORCE 必须对子表数据生效。
///
/// 分区子表本身没有策略（也不该有），如果父表策略没作用到子表，
/// 这里会读到其他租户的审计记录——这是最容易漏的一类。
#[tokio::test]
#[ignore = "需要非超级用户角色（RLS_TEST_DATABASE_URL）"]
async fn partitioned_audit_log_inherits_parent_rls() {
    let pool = probe_pool().await;

    // 无上下文：父表（含所有分区）应为 0 行
    let without: i64 = sqlx::query_scalar("SELECT count(*) FROM sys.sys_audit_log")
        .fetch_one(&pool)
        .await
        .expect("count audit without context");
    assert_eq!(
        without, 0,
        "无租户上下文时审计表返回了 {without} 行——分区子表绕过了父表策略"
    );

    let tenant = any_tenant(&pool).await;
    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");

    let foreign: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sys.sys_audit_log WHERE tenant_id <> $1")
            .bind(tenant)
            .fetch_one(&mut *tx)
            .await
            .expect("count foreign audit rows");
    assert_eq!(foreign, 0, "通过分区表读到了其他租户的审计记录");

    tx.commit().await.expect("commit");
}

/// **表 owner 也必须受约束**——这一条才是 FORCE 的真正验收标准。
///
/// 前置的 `probe_pool` 测试用的是普通角色，即使某表忘了 `FORCE` 也会通过；
/// 这里换成表 owner，`ENABLE`-only 的表会立刻暴露：owner 能看到全部租户的行。
#[tokio::test]
#[ignore = "需要表 owner 角色（RLS_OWNER_DATABASE_URL）"]
async fn table_owner_is_also_restricted_by_force_rls() {
    let pool = owner_pool().await;

    for table in [
        "sys.sys_user",
        "sys.sys_role",
        "sys.sys_menu",
        "sys.sys_dept",
        "sys.sys_config",
        "sys.sys_file",
        "sys.sys_notice",
        "sys.sys_dict_type",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| panic!("查询 {table} 失败: {e}"));
        assert_eq!(
            count, 0,
            "{table} 的表 owner 在无 app.tenant_id 时读到了 {count} 行——该表只 ENABLE 未 FORCE"
        );
    }
}

/// owner 设置租户上下文后，也只能看到本租户的行。
#[tokio::test]
#[ignore = "需要表 owner 角色（RLS_OWNER_DATABASE_URL）"]
async fn table_owner_sees_only_its_tenant_when_context_set() {
    let pool = owner_pool().await;
    let tenant = any_tenant(&pool).await;

    let mut tx = pool.begin().await.expect("begin");
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .expect("set tenant");

    for table in ["sys.sys_user", "sys.sys_role", "sys.sys_menu"] {
        let foreign: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM {table} WHERE tenant_id <> $1"
        ))
        .bind(tenant)
        .fetch_one(&mut *tx)
        .await
        .unwrap_or_else(|e| panic!("查询 {table} 失败: {e}"));
        assert_eq!(foreign, 0, "{table} 的表 owner 看到了其他租户的行");
    }

    tx.commit().await.expect("commit");
}
