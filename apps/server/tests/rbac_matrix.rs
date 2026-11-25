//! RBAC 权限矩阵测试：用最小权限令牌验证关键接口的放行/拒绝。
//!
//! 运行：
//!   $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin"
//!   cargo test -p server --test rbac_matrix -- --ignored --nocapture

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

async fn call_bearer(router: &axum::Router, uri: &str, token: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
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

#[tokio::test]
#[ignore = "需要真实 PostgreSQL"]
async fn rbac_matrix() {
    let url = std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL");
    std::env::set_var("DATABASE_URL", url);
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");

    // 管理员登录
    let login = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "username": "admin", "password": "admin123456", "tenant": null }).to_string(),
        ))
        .unwrap();
    let resp = app.router.clone().oneshot(login).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    // 仅 crm:customer:list 的令牌
    let (status, body) = call(
        &app.router,
        "POST",
        "/api/auth/tokens",
        Some(&cookie),
        Some(json!({
            "name": "rbac-matrix",
            "scopes": ["crm:customer:list"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create token: {body}");
    let token = body["data"]["token"].as_str().unwrap().to_string();
    let token_id = body["data"]["tokenInfo"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let cases = [
        ("GET", "/api/crm/customers?page=1&size=1", true),
        ("GET", "/api/crm/opportunities?page=1&size=1", false),
        ("GET", "/api/crm/contracts?page=1&size=1", false),
        ("GET", "/api/crm/payments?page=1&size=1", false),
        ("GET", "/api/system/users?page=1&size=1", false),
        ("GET", "/api/system/roles", false),
        ("GET", "/api/system/audit-logs?page=1&size=1", false),
        ("GET", "/api/system/audit-config/export", false),
    ];
    for (method, uri, allowed) in cases {
        let (status, body) = call_bearer(&app.router, uri, &token).await;
        if allowed {
            assert_eq!(status, StatusCode::OK, "{method} {uri} 应放行: {body}");
        } else {
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri} 应拒绝");
        }
        println!("{method} {uri} -> {status}");
    }

    // 写接口必须拒绝（DELETE 不存在 id，若权限通过会是 404 而非 403）
    let delete_uri = "/api/crm/customers/00000000-0000-0000-0000-000000000000".to_string();
    let req = Request::builder()
        .method("DELETE")
        .uri(&delete_uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let _ = call(
        &app.router,
        "DELETE",
        &format!("/api/auth/tokens/{token_id}"),
        Some(&cookie),
        None,
    )
    .await;
    println!("rbac_matrix: passed");
}
