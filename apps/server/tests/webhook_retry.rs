//! Webhook 失败退避测试：不可达地址 → 记录 attempts / next_notify_at。
//!
//! 运行：
//!   $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin"
//!   cargo test -p server --test webhook_retry -- --ignored --nocapture

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

use plugin_crm as _;
use plugin_demo as _;

async fn call(
    router: &axum::Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(c) = cookie {
        builder = builder.header(header::COOKIE, c);
    }
    let req = match body {
        Some(v) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let resp = router.clone().oneshot(req).await.expect("request");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn login_cookie(router: &axum::Router, username: &str, password: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "username": username, "password": password, "tenant": null }).to_string(),
        ))
        .unwrap();
    let resp = router.clone().oneshot(req).await.expect("login");
    assert_eq!(resp.status(), StatusCode::OK, "login failed");
    resp.headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie")
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

#[tokio::test]
#[ignore = "需要真实 PostgreSQL"]
async fn webhook_failure_backs_off() {
    let url = std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL");
    std::env::set_var("DATABASE_URL", url);
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");
    let cookie = login_cookie(&app.router, "admin", "admin123456").await;

    // 不可达地址（端口 1）
    let (status, body) = call(
        &app.router,
        "PUT",
        "/api/system/audit-webhook",
        Some(&cookie),
        Some(json!({
            "url": "http://127.0.0.1:1/hook",
            "enabled": true,
            "secret": "retry-secret"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "webhook config failed: {body}");

    let tenant: sqlx::types::Uuid = sqlx::query_scalar("SELECT id FROM sys.sys_tenant LIMIT 1")
        .fetch_one(&app.state.db)
        .await
        .unwrap();
    let mut tx = app.state.db.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE sys.sys_audit_alert SET notified_at = now() WHERE tenant_id = $1 AND notified_at IS NULL")
        .bind(tenant)
        .execute(&mut *tx)
        .await
        .unwrap();
    let alert_id: sqlx::types::Uuid = sqlx::query_scalar(
        "INSERT INTO sys.sys_audit_alert
            (id, tenant_id, action, window_start, window_end, count, threshold)
         VALUES (gen_random_uuid(), $1, 'retry.test', now() - interval '5 minutes', now(), 99, 50)
         RETURNING id",
    )
    .bind(tenant)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let notified = kernel::audit::notify_pending(&app.state.db).await.unwrap();
    assert_eq!(notified, 0, "unreachable webhook should not succeed");

    let mut tx = app.state.db.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let row: (i32, bool) = sqlx::query_as(
        "SELECT notify_attempts, next_notify_at > now() FROM sys.sys_audit_alert WHERE id = $1",
    )
    .bind(alert_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(row.0, 1, "attempts should be 1");
    assert!(row.1, "next_notify_at should be in the future");

    // 未到重试时间：不应再次尝试
    let again = kernel::audit::notify_pending(&app.state.db).await.unwrap();
    assert_eq!(again, 0);
    let attempts: i32 = {
        let mut tx = app.state.db.begin().await.unwrap();
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let a: i32 =
            sqlx::query_scalar("SELECT notify_attempts FROM sys.sys_audit_alert WHERE id = $1")
                .bind(alert_id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        tx.commit().await.unwrap();
        a
    };
    assert_eq!(attempts, 1, "backoff should skip early retry");

    // 第 8 次失败应进入死信
    let mut tx = app.state.db.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE sys.sys_audit_alert
         SET notify_attempts = 7, next_notify_at = now() - interval '1 second', notify_failed_at = NULL
         WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant)
    .bind(alert_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let _ = kernel::audit::notify_pending(&app.state.db).await.unwrap();
    let mut tx = app.state.db.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let (attempts, dead): (i32, bool) = sqlx::query_as(
        "SELECT notify_attempts, notify_failed_at IS NOT NULL FROM sys.sys_audit_alert WHERE id = $1",
    )
    .bind(alert_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(attempts, 8);
    assert!(dead, "8th failure should mark dead letter");

    // 清理：关闭 webhook，避免影响其他测试
    let _ = call(
        &app.router,
        "PUT",
        "/api/system/audit-webhook",
        Some(&cookie),
        Some(json!({ "url": "", "enabled": false, "secret": null })),
    )
    .await;

    println!("webhook_failure_backs_off: passed");
}
