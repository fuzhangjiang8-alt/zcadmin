//! Webhook 投递端到端测试：本地 axum 接收端 + 签名校验。
//!
//! 运行：
//!   $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin"
//!   cargo test -p server --test webhook_delivery -- --ignored --nocapture

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::routing::post;
use axum::Router;
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

use plugin_crm as _;
use plugin_demo as _;

type Captured = Arc<Mutex<Option<(String, String)>>>;

async fn hook(State(captured): State<Captured>, headers: HeaderMap, body: String) -> StatusCode {
    let signature = headers
        .get("x-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    *captured.lock().unwrap() = Some((body, signature));
    StatusCode::OK
}

async fn call(
    router: &Router,
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

async fn login_cookie(router: &Router, username: &str, password: &str) -> String {
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
async fn webhook_delivers_and_signs() {
    let url = std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL");
    std::env::set_var("DATABASE_URL", url);
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");
    let cookie = login_cookie(&app.router, "admin", "admin123456").await;

    // 本地接收端
    let captured: Captured = Arc::new(Mutex::new(None));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().unwrap();
    let hook_app = Router::new()
        .route("/hook", post(hook))
        .with_state(captured.clone());
    tokio::spawn(async move {
        axum::serve(listener, hook_app).await.unwrap();
    });

    // 启用 Webhook
    let secret = "test-secret";
    let (status, body) = call(
        &app.router,
        "PUT",
        "/api/system/audit-webhook",
        Some(&cookie),
        Some(json!({
            "url": format!("http://{addr}/hook"),
            "enabled": true,
            "secret": secret
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "webhook config failed: {body}");

    // 直接插入一条未通知告警
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
    sqlx::query(
        "UPDATE sys.sys_audit_alert SET notified_at = now() WHERE tenant_id = $1 AND notified_at IS NULL",
    )
    .bind(tenant)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sys.sys_audit_alert
            (id, tenant_id, action, window_start, window_end, count, threshold)
         VALUES (gen_random_uuid(), $1, 'auth.login.failed', now() - interval '5 minutes', now(), 99, 50)",
    )
    .bind(tenant)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let notified = kernel::audit::notify_pending(&app.state.db).await.unwrap();
    assert!(notified >= 1, "expected at least one webhook delivery");

    // 等待接收端记录
    let mut payload = None;
    for _ in 0..40 {
        if let Some(v) = captured.lock().unwrap().clone() {
            payload = Some(v);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let (body_text, signature) = payload.expect("webhook not received");
    assert!(body_text.contains("auth.login.failed"), "body: {body_text}");

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body_text.as_bytes());
    let expected: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(signature, format!("sha256={expected}"));

    // 已标记通知，再跑不应重复投递
    let again = kernel::audit::notify_pending(&app.state.db).await.unwrap();
    assert_eq!(again, 0, "alert should not be delivered twice");

    println!("webhook_delivers_and_signs: passed");
}
