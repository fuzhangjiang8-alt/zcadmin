//! 数据库连接与迁移执行。
//!
//! ## 两个关键约束
//!
//! 1. **每个插件一个 schema**（见 `docs/03-数据架构.md`）。
//!    插件只能写自己的 schema，跨插件读取必须走对方提供的只读视图。
//! 2. **迁移串行执行**。多实例同时启动时，用 PostgreSQL 咨询锁（advisory lock）保证
//!    同一时刻只有一个实例在跑 DDL，其余实例等待。
//! 3. **每条连接都设 `statement_timeout`**。客户端超时/断连后 PostgreSQL 仍会继续
//!    执行查询，只有服务端超时才真正释放 CPU 与锁。

use crate::config::DatabaseConfig;
use anyhow::Context;
use sqlx::migrate::Migrator;
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Duration;

/// 建立连接池。
///
/// 连接池大小与超时来自配置；`acquire_timeout` 很重要：
/// 池满时应该快速失败（503），而不是让请求无限排队把线程耗光。
///
/// 每建立一条新连接都会执行：
/// - `SET statement_timeout`：服务端强制终止超时查询（核心防雪崩措施）；
/// - `SET idle_in_transaction_session_timeout`：事务开着却不干活的连接自动断开，
///   防止业务代码忘记 commit 长期占住锁与连接。
pub async fn connect(cfg: &DatabaseConfig) -> anyhow::Result<PgPool> {
    let statement_timeout = format!("{}s", cfg.statement_timeout_secs);
    // 事务开着却不干活 = 业务代码漏了 commit。预留 30s 缓冲，避免把慢查询误杀。
    let idle_tx_timeout = format!("{}s", (cfg.statement_timeout_secs * 6).max(30));

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(cfg.max_connections)
        .min_connections(cfg.min_connections.min(cfg.max_connections))
        .acquire_timeout(Duration::from_secs(cfg.acquire_timeout_secs))
        .max_lifetime(Duration::from_secs(cfg.max_lifetime_secs))
        .idle_timeout(Some(Duration::from_secs(600)))
        .after_connect(move |conn, _meta| {
            let statement_timeout = statement_timeout.clone();
            let idle_tx_timeout = idle_tx_timeout.clone();
            Box::pin(async move {
                sqlx::query("SELECT set_config('statement_timeout', $1, false)")
                    .bind(&statement_timeout)
                    .execute(&mut *conn)
                    .await?;
                sqlx::query("SELECT set_config('idle_in_transaction_session_timeout', $1, false)")
                    .bind(&idle_tx_timeout)
                    .execute(&mut *conn)
                    .await?;
                Ok(())
            })
        })
        .connect(&cfg.url)
        .await
        .with_context(|| "连接 PostgreSQL 失败，请检查 DATABASE_URL 与数据库是否已启动")?;

    tracing::info!(
        max_connections = cfg.max_connections,
        min_connections = cfg.min_connections,
        statement_timeout_secs = cfg.statement_timeout_secs,
        "数据库连接池已建立"
    );
    Ok(pool)
}

/// 校验 schema 名是否合法。
///
/// schema 名会拼进 SQL（DDL 不支持参数绑定），因此必须严格限制字符集，
/// 只允许小写字母、数字和下划线。这是防注入的最后一道闸门。
fn validate_identifier(name: &str) -> anyhow::Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 63
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    anyhow::ensure!(ok, "非法的 schema 名称: {name}");
    Ok(())
}

/// 确保 schema 存在（不存在则创建）。
pub async fn ensure_schema(pool: &PgPool, schema: &str) -> anyhow::Result<()> {
    validate_identifier(schema)?;
    let sql = format!("CREATE SCHEMA IF NOT EXISTS \"{schema}\"");
    sqlx::query(&sql)
        .execute(pool)
        .await
        .with_context(|| format!("创建 schema 失败: {schema}"))?;
    Ok(())
}

/// 在指定 schema 下执行迁移。
///
/// 原理：先 `SET search_path`，再运行 `sqlx::migrate!` 生成的 [`Migrator`]。
/// `_sqlx_migrations` 版本表会落在该 schema 内，天然做到「每插件独立迁移历史」。
pub async fn run_migrations(
    pool: &PgPool,
    schema: &str,
    migrator: &Migrator,
) -> anyhow::Result<()> {
    validate_identifier(schema)?;

    // 咨询锁：key 用 schema 名做哈希，保证不同插件互不阻塞、同一插件只跑一次。
    let lock_key = advisory_key(schema);
    let mut conn = pool.acquire().await?;

    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(lock_key)
        .execute(&mut *conn)
        .await
        .context("获取迁移锁失败")?;

    let result = async {
        // 迁移里的 DDL 可能长时间持锁（大表加列、建索引）。连接级的
        // statement_timeout（默认 5s）会把迁移直接掐断，因此这里临时放开，
        // 结束后必须恢复——连接会回池复用，设置是会话级的。
        let previous_timeout: String =
            sqlx::query_scalar("SELECT current_setting('statement_timeout')")
                .fetch_one(&mut *conn)
                .await
                .context("读取 statement_timeout 失败")?;
        sqlx::query("SET statement_timeout = 0")
            .execute(&mut *conn)
            .await?;

        let set_path = format!("SET search_path TO \"{schema}\"");
        sqlx::query(&set_path).execute(&mut *conn).await?;
        migrator.run(&mut *conn).await?;

        // 还原 search_path 与超时，避免影响连接池中后续复用该连接的查询。
        sqlx::query("SET search_path TO public")
            .execute(&mut *conn)
            .await?;
        sqlx::query("SELECT set_config('statement_timeout', $1, false)")
            .bind(&previous_timeout)
            .execute(&mut *conn)
            .await?;
        Ok::<(), anyhow::Error>(())
    }
    .await;

    // 无论成功失败都要解锁，否则下次启动会永久卡住。
    let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(lock_key)
        .execute(&mut *conn)
        .await;

    result.with_context(|| format!("执行迁移失败: schema={schema}"))?;
    tracing::info!(schema, "迁移执行完成");
    Ok(())
}

/// 把 schema 名映射成 64 位咨询锁 key。
fn advisory_key(schema: &str) -> i64 {
    // FNV-1a：简单、稳定、无依赖。不同 schema 冲突概率极低。
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in schema.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash as i64
}

/// 在事务中执行一段逻辑的便捷封装。
///
/// 说明：这里只做最薄的一层包装，事务的**边界**应由 service 层显式决定，
/// 不要让 repo 层偷偷开事务（会导致跨多次调用的隐式长事务）。
pub async fn with_tx<'a, T, F>(pool: &'a PgPool, f: F) -> anyhow::Result<T>
where
    F: for<'c> FnOnce(
        &'c mut Transaction<'a, Postgres>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<T>> + Send + 'c>,
    >,
{
    let mut tx = pool.begin().await?;
    let out = f(&mut tx).await?;
    tx.commit().await?;
    Ok(out)
}
