//! 会话真库回归：sqlx 为每个用例创建并销毁独立数据库，不使用现有业务库中的账号。
//! 运行：设置 DATABASE_URL 后 cargo test -p kernel --test session_atomicity -- --ignored。

use kernel::session::{Session, SessionStore};
use sqlx::{migrate::Migrator, PgPool};
use std::sync::Arc;
use uuid::Uuid;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// 使用正式迁移，避免测试自建表遗漏线上约束；返回无外键依赖的会话快照。
async fn prepare(pool: &PgPool) -> Session {
    kernel::db::ensure_schema(pool, "sys")
        .await
        .expect("schema");
    kernel::db::run_migrations(pool, "sys", &MIGRATOR)
        .await
        .expect("migrate");
    Session {
        id: Uuid::now_v7(),
        tenant_id: Uuid::now_v7(),
        user_id: Uuid::now_v7(),
        dept_id: None,
        ip: None,
        user_agent: None,
        data_scope: "self".into(),
        username: "concurrent-user".into(),
        permissions: vec![],
        created_at: sqlx::query_scalar("SELECT now()")
            .fetch_one(pool)
            .await
            .expect("created"),
        expires_at: sqlx::query_scalar("SELECT now() + interval '1 hour'")
            .fetch_one(pool)
            .await
            .expect("expires"),
        must_change_password: false,
    }
}

#[sqlx::test(migrations = false)]
#[ignore = "需要 PostgreSQL 及创建测试数据库权限"]
async fn concurrent_single_session_across_stores_keeps_one_token(pool: PgPool) {
    let session = prepare(&pool).await;
    // 放大 DELETE 与 INSERT 之间的竞态；旧实现会在全部删除完成后插入多行。
    sqlx::raw_sql(
        "CREATE FUNCTION sys.delay_session_insert() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM pg_sleep(0.05); RETURN NEW; END $$;
        CREATE TRIGGER delay_session BEFORE INSERT ON sys.sys_session
        FOR EACH ROW EXECUTE FUNCTION sys.delay_session_insert();",
    )
    .execute(&pool)
    .await
    .expect("delay trigger");
    let barrier = Arc::new(tokio::sync::Barrier::new(8));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let store = SessionStore::new(pool.clone());
        let mut session = session.clone();
        session.id = Uuid::now_v7();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            store.create(session, true).await.expect("create")
        }));
    }
    let mut tokens = Vec::new();
    for task in tasks {
        tokens.push(task.await.expect("join"));
    }
    let store = SessionStore::new(pool);
    let mut valid = 0;
    for token in tokens {
        if store.get(&token).await.expect("lookup").is_some() {
            valid += 1;
        }
    }
    assert_eq!(
        store.list(session.tenant_id).await.expect("sessions").len(),
        1
    );
    assert_eq!(valid, 1);
}

#[sqlx::test(migrations = false)]
#[ignore = "需要 PostgreSQL 及创建测试数据库权限"]
async fn failed_replacement_rolls_back_revocation(pool: PgPool) {
    let session = prepare(&pool).await;
    let store = SessionStore::new(pool.clone());
    let original = store
        .create(session.clone(), true)
        .await
        .expect("first session");
    let mut invalid = session.clone();
    invalid.id = Uuid::now_v7();
    invalid.ip = Some("not-an-ip".into());
    assert!(store.create(invalid, true).await.is_err());
    let fresh = SessionStore::new(pool);
    assert!(fresh
        .get(&original)
        .await
        .expect("original survived")
        .is_some());
    assert_eq!(
        fresh.list(session.tenant_id).await.expect("sessions").len(),
        1
    );
}

#[sqlx::test(migrations = false)]
#[ignore = "需要 PostgreSQL 及创建测试数据库权限"]
async fn multiple_sessions_and_tenant_boundaries_are_preserved(pool: PgPool) {
    let session = prepare(&pool).await;
    let store = SessionStore::new(pool);
    let first = store.create(session.clone(), false).await.expect("first");
    let mut second = session.clone();
    second.id = Uuid::now_v7();
    let second = store.create(second, false).await.expect("second");
    let mut other_tenant = session.clone();
    other_tenant.id = Uuid::now_v7();
    other_tenant.tenant_id = Uuid::now_v7();
    store
        .create(other_tenant, true)
        .await
        .expect("other tenant");
    assert_eq!(
        store.list(session.tenant_id).await.expect("sessions").len(),
        2
    );
    assert!(store.get(&first).await.expect("first valid").is_some());
    assert!(store.get(&second).await.expect("second valid").is_some());
}
