//! 基础用户导入真库回归：正式迁移与真实上传路由，验证数据库异常后的部分成功。
//! sqlx 为每个用例创建测试库；临时工作簿与文件目录不会改动既有租户或账号。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use kernel::{config::Config, session::Session, AppState};
use sqlx::{migrate::Migrator, PgPool};
use tower::ServiceExt;
use uuid::Uuid;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// 创建隔离租户与有用户新增权限的会话，避免初始化管理员和插件影响断言。
async fn prepare(pool: PgPool) -> (AppState, Session, String) {
    kernel::db::ensure_schema(&pool, "sys")
        .await
        .expect("schema");
    kernel::db::run_migrations(&pool, "sys", &MIGRATOR)
        .await
        .expect("migrate");
    let tenant = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO sys.sys_tenant (id, code, name) VALUES ($1, 'import-test', 'import test')",
    )
    .bind(tenant)
    .execute(&pool)
    .await
    .expect("tenant");
    let mut config = Config::default();
    config.storage.local_dir = std::env::temp_dir()
        .join(format!("zcadmin-import-test-{}", Uuid::new_v4()))
        .to_string_lossy()
        .into_owned();
    let state = AppState::new(pool, config).expect("state");
    let session = session(&state, tenant).await;
    let token = state
        .sessions
        .create(session.clone(), false)
        .await
        .expect("session");
    (state, session, token)
}

/// 只授予本次接口所需权限；会话本身无需落库用户外键。
async fn session(state: &AppState, tenant_id: Uuid) -> Session {
    Session {
        id: Uuid::now_v7(),
        tenant_id,
        user_id: Uuid::now_v7(),
        dept_id: None,
        ip: None,
        user_agent: None,
        data_scope: "self".into(),
        username: "importer".into(),
        permissions: vec![kernel::perms::USER_ADD.into()],
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

/// 生成三行有效输入；中间行由数据库注入约束失败，后续行应继续写入。
fn upload_body() -> Vec<u8> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook.add_worksheet();
    for (column, title) in ["用户名", "显示名", "初始密码"].iter().enumerate() {
        sheet
            .write_string(0, column as u16, *title)
            .expect("header");
    }
    for (index, username) in ["first_user", "reject_row", "last_user"].iter().enumerate() {
        sheet
            .write_string((index + 1) as u32, 0, *username)
            .expect("username");
        sheet
            .write_string((index + 1) as u32, 1, *username)
            .expect("name");
        sheet
            .write_string((index + 1) as u32, 2, "User#12345")
            .expect("password");
    }
    let mut body = b"--import-test\r\nContent-Disposition: form-data; name=file; filename=users.xlsx\r\nContent-Type: application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\r\n\r\n".to_vec();
    body.extend(workbook.save_to_buffer().expect("xlsx"));
    body.extend(b"\r\n--import-test--\r\n");
    body
}

/// 走完整提取器与导入处理器，不直接调用插入辅助函数。
async fn import(state: &AppState, token: &str, dry_run: bool) -> serde_json::Value {
    let router = kernel::user_import::routes().with_state(state.clone());
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/system/users/import?dryRun={dry_run}"))
        .header(
            header::COOKIE,
            format!("{}={token}", kernel::auth::SESSION_COOKIE),
        )
        .header(
            header::CONTENT_TYPE,
            "multipart/form-data; boundary=import-test",
        )
        .body(Body::from(upload_body()))
        .expect("request");
    let response = router.oneshot(request).await.expect("response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 65536)
        .await
        .expect("body");
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    serde_json::from_slice(&body).expect("response JSON")
}

#[sqlx::test(migrations = false)]
#[ignore = "需要 PostgreSQL 及创建测试数据库权限"]
async fn database_error_isolated_to_one_row_preserves_partial_success(pool: PgPool) {
    let (state, session, token) = prepare(pool).await;
    sqlx::raw_sql("CREATE FUNCTION sys.reject_import_row() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN IF NEW.username = 'reject_row' THEN RAISE EXCEPTION 'injected row rejection' USING ERRCODE = '23514';
        END IF; RETURN NEW; END $$;
        CREATE TRIGGER reject_import_row BEFORE INSERT ON sys.sys_user
        FOR EACH ROW EXECUTE FUNCTION sys.reject_import_row();")
        .execute(&state.db).await.expect("trigger");
    let response = import(&state, &token, false).await;
    assert_eq!(response["data"]["total"], 3);
    assert_eq!(response["data"]["success"], 2);
    assert_eq!(response["data"]["failed"], 1);
    assert_eq!(response["data"]["errors"][0]["row"], 3);
    assert!(!response.to_string().contains("injected row rejection"));
    let mut tx = kernel::tenant::TenantTx::begin_tenant(&state.db, session.tenant_id)
        .await
        .expect("tx");
    let names: Vec<String> =
        sqlx::query_scalar("SELECT username FROM sys.sys_user ORDER BY username")
            .fetch_all(&mut *tx)
            .await
            .expect("stored rows");
    assert_eq!(names, vec!["first_user", "last_user"]);
    tx.commit().await.expect("commit");
    tokio::fs::remove_dir_all(&state.config.storage.local_dir)
        .await
        .expect("cleanup");
}

#[sqlx::test(migrations = false)]
#[ignore = "需要 PostgreSQL 及创建测试数据库权限"]
async fn dry_run_does_not_insert_users(pool: PgPool) {
    let (state, session, token) = prepare(pool).await;
    let response = import(&state, &token, true).await;
    assert_eq!(response["data"]["success"], 3);
    assert_eq!(response["data"]["failed"], 0);
    let mut tx = kernel::tenant::TenantTx::begin_tenant(&state.db, session.tenant_id)
        .await
        .expect("tx");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sys.sys_user")
        .fetch_one(&mut *tx)
        .await
        .expect("count");
    assert_eq!(count, 0);
    tx.commit().await.expect("commit");
    tokio::fs::remove_dir_all(&state.config.storage.local_dir)
        .await
        .expect("cleanup");
}
