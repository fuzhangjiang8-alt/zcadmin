//! CSRF 中间件集成测试（真库，直接走 axum Router）。
//!
//! 验证：
//! - 跨站来源（`Origin` 与 `Host` 不同源）的写请求被 403 拒绝；
//! - 同源来源的写请求正常进入业务逻辑；
//! - 无 `Origin` 的调用（服务端/脚本）不被拦截。
//!
//! 运行：
//! ```powershell
//! $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin_csrf"
//! cargo test -p server --test csrf_guard -- --ignored
//! ```

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use serde_json::json;
use tower::ServiceExt;

use plugin_crm as _;
use plugin_demo as _;

fn login_request(origin: Option<&str>, password: &str) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::HOST, "localhost:8080");
    if let Some(origin) = origin {
        builder = builder.header(header::ORIGIN, origin);
    }
    builder
        .body(Body::from(
            json!({ "username": "admin", "password": password, "tenant": null }).to_string(),
        ))
        .unwrap()
}

#[tokio::test]
#[ignore = "需要真实 PostgreSQL（DATABASE_URL）"]
async fn csrf_guard_blocks_cross_site() {
    std::env::set_var(
        "DATABASE_URL",
        std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL"),
    );
    let password = std::env::var("AUTH_DEV_PASSWORD").unwrap_or_else(|_| "admin123456".into());
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");

    // 跨站来源：403，且在进入登录逻辑前就被拦截
    let response = app
        .router
        .clone()
        .oneshot(login_request(Some("https://evil.example.com"), &password))
        .await
        .expect("cross-site request");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "cross-site must be rejected"
    );

    // 同源来源：正常登录
    let response = app
        .router
        .clone()
        .oneshot(login_request(Some("http://localhost:8080"), &password))
        .await
        .expect("same-origin request");
    assert_eq!(response.status(), StatusCode::OK, "same-origin must pass");

    // 无 Origin（服务端/脚本调用）：正常登录
    let response = app
        .router
        .clone()
        .oneshot(login_request(None, &password))
        .await
        .expect("no-origin request");
    assert_eq!(response.status(), StatusCode::OK, "no origin must pass");
}
