//! 单点登录（single_session）集成测试。
//!
//! 运行（必须单线程：两个用例共用 admin 账号，并行会互相吊销会话）：
//!   $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin"
//!   cargo test -p server --test single_session -- --ignored --nocapture --test-threads=1

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use plugin_crm as _;
use plugin_demo as _;

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
    assert_eq!(resp.status(), StatusCode::OK, "login failed for {username}");
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

async fn me(router: &axum::Router, cookie: &str) -> StatusCode {
    let req = Request::builder()
        .method("GET")
        .uri("/api/auth/me")
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap();
    router.clone().oneshot(req).await.expect("me").status()
}

/// 带 JSON 的通用请求，返回 (状态码, 响应体)。
async fn call(
    router: &axum::Router,
    method: &str,
    uri: &str,
    cookie: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie);
    let req = match body {
        Some(v) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let resp = router.clone().oneshot(req).await.expect("request");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
#[ignore = "需要真实 PostgreSQL"]
async fn single_session_kicks_previous() {
    let url = std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL");
    std::env::set_var("DATABASE_URL", url);
    let mut config = kernel::config::Config::load("config/default.toml").expect("config");
    config.auth.single_session = true;
    let app = kernel::app::build(config).await.expect("build app");

    let first = login_cookie(&app.router, "admin", "admin123456").await;
    assert_eq!(me(&app.router, &first).await, StatusCode::OK);

    let second = login_cookie(&app.router, "admin", "admin123456").await;
    assert_eq!(
        me(&app.router, &first).await,
        StatusCode::UNAUTHORIZED,
        "first session should be revoked"
    );
    assert_eq!(me(&app.router, &second).await, StatusCode::OK);

    println!("single_session_kicks_previous: passed");
}

/// 禁用用户必须立刻吊销其会话。
///
/// 回归背景：`update_user_status` 曾只改 `sys_user.status`，会话里的权限快照
/// 仍然有效，被禁用的人最长还能操作 8 小时。
#[tokio::test]
#[ignore = "需要真实 PostgreSQL"]
async fn disabled_user_session_is_revoked() {
    let url = std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL");
    std::env::set_var("DATABASE_URL", url);
    let admin_password =
        std::env::var("AUTH_DEV_PASSWORD").unwrap_or_else(|_| "admin123456".into());
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");

    let admin = login_cookie(&app.router, "admin", &admin_password).await;
    assert_eq!(me(&app.router, &admin).await, StatusCode::OK);

    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    let username = format!("revoke{suffix}");

    let (status, body) = call(
        &app.router,
        "POST",
        "/api/system/users",
        &admin,
        Some(json!({
            "username": username,
            "displayName": "被禁用用户",
            "password": "User#12345",
            "mustChangePassword": false
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create user failed: {body}");
    let user_id = body["data"]["id"].as_str().expect("user id").to_string();

    let user_cookie = login_cookie(&app.router, &username, "User#12345").await;
    assert_eq!(me(&app.router, &user_cookie).await, StatusCode::OK);

    let (status, body) = call(
        &app.router,
        "PATCH",
        &format!("/api/system/users/{user_id}/status"),
        &admin,
        Some(json!({ "status": "disabled" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "disable user failed: {body}");

    assert_eq!(
        me(&app.router, &user_cookie).await,
        StatusCode::UNAUTHORIZED,
        "禁用后旧会话必须立刻失效"
    );

    println!("disabled_user_session_is_revoked: passed");
}
