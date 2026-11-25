//! 文件删除真库回归：正式迁移 + 真实 Router + 临时磁盘目录，注入延迟提交失败。
//! 每个 sqlx 用例使用独立数据库；不启动插件、后台任务或改动既有业务数据。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use kernel::{config::Config, session::Session, AppState};
use sqlx::{migrate::Migrator, PgPool};
use tower::ServiceExt;
use uuid::Uuid;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

struct Fixture {
    state: AppState,
    router: axum::Router,
    session: Session,
    token: String,
    file_id: Uuid,
    key: String,
    root: std::path::PathBuf,
}

/// 使用正式迁移和临时文件验证事务边界，不启动插件或应用后台任务。
async fn prepare(pool: PgPool) -> Fixture {
    kernel::db::ensure_schema(&pool, "sys")
        .await
        .expect("schema");
    kernel::db::run_migrations(&pool, "sys", &MIGRATOR)
        .await
        .expect("migrate");
    let tenant = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO sys.sys_tenant (id, code, name) VALUES ($1, 'file-test', 'file test')",
    )
    .bind(tenant)
    .execute(&pool)
    .await
    .expect("tenant");
    let mut config = Config::default();
    let root = std::env::temp_dir().join(format!("zcadmin-file-test-{}", Uuid::new_v4()));
    config.storage.local_dir = root.to_string_lossy().into_owned();
    let state = AppState::new(pool, config).expect("state");
    let session = make_session(&state, tenant).await;
    let token = state
        .sessions
        .create(session.clone(), false)
        .await
        .expect("session");
    let file_id = Uuid::now_v7();
    let key = format!("{tenant}/{file_id}");
    save_file(&state, &session, file_id, &key).await;
    let router = kernel::files::routes().with_state(state.clone());
    Fixture {
        state,
        router,
        session,
        token,
        file_id,
        key,
        root,
    }
}

/// 仅授予文件删除权限，读取由上传者归属判断放行。
async fn make_session(state: &AppState, tenant_id: Uuid) -> Session {
    Session {
        id: Uuid::now_v7(),
        tenant_id,
        user_id: Uuid::now_v7(),
        dept_id: None,
        ip: None,
        user_agent: None,
        data_scope: "self".into(),
        username: "file-user".into(),
        permissions: vec![kernel::perms::FILE_DELETE.into()],
        must_change_password: false,
        created_at: sqlx::query_scalar("SELECT now()")
            .fetch_one(&state.db)
            .await
            .expect("created"),
        expires_at: sqlx::query_scalar("SELECT now() + interval '1 hour'")
            .fetch_one(&state.db)
            .await
            .expect("expires"),
    }
}

/// 用真实存储写入内容，再在租户上下文中创建关联元数据。
async fn save_file(state: &AppState, session: &Session, file_id: Uuid, key: &str) {
    state
        .storage
        .save(key, b"original file")
        .await
        .expect("save");
    let mut tx = kernel::tenant::TenantTx::begin_tenant(&state.db, session.tenant_id)
        .await
        .expect("tenant tx");
    sqlx::query(
        "INSERT INTO sys.sys_file (id, tenant_id, name, mime, size_bytes, storage_key, uploader_id)
        VALUES ($1, $2, 'test.txt', 'text/plain', 13, $3, $4)",
    )
    .bind(file_id)
    .bind(session.tenant_id)
    .bind(key)
    .bind(session.user_id)
    .execute(&mut *tx)
    .await
    .expect("metadata");
    tx.commit().await.expect("commit");
}

/// 通过公开路由发送 Cookie 请求，保留认证和 TenantTx 提取器。
async fn call(
    fixture: &Fixture,
    method: &str,
    content: bool,
    token: &str,
) -> axum::response::Response {
    let suffix = if content { "/content" } else { "" };
    let request = Request::builder()
        .method(method)
        .uri(format!("/api/system/files/{}{suffix}", fixture.file_id))
        .header(
            header::COOKIE,
            format!("{}={token}", kernel::auth::SESSION_COOKIE),
        )
        .body(Body::empty())
        .expect("request");
    fixture
        .router
        .clone()
        .oneshot(request)
        .await
        .expect("response")
}

/// 验证下载和物理内容均完好，覆盖「元数据提交失败但文件已删」的旧故障。
async fn assert_readable(fixture: &Fixture) {
    let response = call(fixture, "GET", true, &fixture.token).await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1024)
        .await
        .expect("body");
    assert_eq!(&bytes[..], b"original file");
    assert_eq!(
        fixture
            .state
            .storage
            .load(&fixture.key)
            .await
            .expect("physical file"),
        b"original file"
    );
}

#[sqlx::test(migrations = false)]
#[ignore = "需要 PostgreSQL 及创建测试数据库权限"]
async fn failed_commit_preserves_metadata_and_physical_file(pool: PgPool) {
    let fixture = prepare(pool).await;
    sqlx::raw_sql(
        "CREATE FUNCTION sys.fail_file_commit() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'injected commit failure'; RETURN NEW; END $$;
        CREATE CONSTRAINT TRIGGER fail_file_commit AFTER UPDATE ON sys.sys_file
        DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION sys.fail_file_commit();",
    )
    .execute(&fixture.state.db)
    .await
    .expect("deferred failure trigger");
    assert_eq!(
        call(&fixture, "DELETE", false, &fixture.token)
            .await
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_readable(&fixture).await;
    tokio::fs::remove_dir_all(&fixture.root)
        .await
        .expect("remove test directory");
}

#[sqlx::test(migrations = false)]
#[ignore = "需要 PostgreSQL 及创建测试数据库权限"]
async fn successful_delete_hides_metadata_and_removes_content(pool: PgPool) {
    let fixture = prepare(pool).await;
    assert_eq!(
        call(&fixture, "DELETE", false, &fixture.token)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        call(&fixture, "GET", true, &fixture.token).await.status(),
        StatusCode::NOT_FOUND
    );
    assert!(fixture.state.storage.load(&fixture.key).await.is_err());
    tokio::fs::remove_dir_all(&fixture.root)
        .await
        .expect("remove test directory");
}

#[sqlx::test(migrations = false)]
#[ignore = "需要 PostgreSQL 及创建测试数据库权限"]
async fn missing_delete_permission_preserves_content(pool: PgPool) {
    let fixture = prepare(pool).await;
    let mut session = fixture.session.clone();
    session.id = Uuid::now_v7();
    session.permissions.clear();
    let token = fixture
        .state
        .sessions
        .create(session, false)
        .await
        .expect("limited session");
    assert_eq!(
        call(&fixture, "DELETE", false, &token).await.status(),
        StatusCode::FORBIDDEN
    );
    assert_readable(&fixture).await;
    tokio::fs::remove_dir_all(&fixture.root)
        .await
        .expect("remove test directory");
}
