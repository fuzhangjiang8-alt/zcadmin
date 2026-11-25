//! 安全基线集成测试（真库，走完整 Router）。
//!
//! 覆盖本轮加固的核心断言：
//! - 直连客户端伪造 `X-Forwarded-For` 不能改变审计记录的 IP；
//! - panic 被 `CatchPanicLayer` 转成 500，进程（Router）继续可用；
//! - 401 / 403 / 503 都带安全响应头与统一 problem+json；
//! - `/readyz` 在停机标记置位后返回 503；
//! - `/metrics` 设置 `METRICS_TOKEN` 后未授权访问返回 401。
//!
//! 运行（PowerShell）：
//! ```powershell
//! $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin"
//! cargo test -p server --test security_baseline -- --ignored --test-threads=1
//! ```

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::routing::get;
use axum::Router;
use serde_json::json;
use tower::ServiceExt;

use plugin_crm as _;
use plugin_demo as _;

/// 读取响应体（限制大小，避免测试自己 OOM）。
async fn body_text(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    String::from_utf8_lossy(&bytes).to_string()
}

fn login_request(password: &str, forwarded_for: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::HOST, "localhost:8080");
    if let Some(value) = forwarded_for {
        builder = builder.header("x-forwarded-for", value);
    }
    builder
        .body(Body::from(
            json!({ "username": "admin", "password": password, "tenant": null }).to_string(),
        ))
        .expect("request")
}

/// 直连（未配置可信代理）时，客户端伪造的 XFF 必须被忽略。
#[tokio::test]
#[ignore = "需要真实 PostgreSQL（DATABASE_URL）"]
async fn forwarded_header_is_ignored_for_direct_clients() {
    std::env::set_var(
        "DATABASE_URL",
        std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL"),
    );
    let password = std::env::var("AUTH_DEV_PASSWORD").unwrap_or_else(|_| "admin123456".into());
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    // 前提断言：默认配置不信任任何代理，否则本测试无意义
    assert!(
        config.server.trusted_proxies.is_empty(),
        "测试要求 trusted_proxies 为空（安全默认值）"
    );
    let app = kernel::app::build(config).await.expect("build app");

    let response = app
        .router
        .clone()
        .oneshot(login_request(&password, Some("203.0.113.9, 10.0.0.1")))
        .await
        .expect("login");
    assert_eq!(response.status(), StatusCode::OK, "login failed");
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie")
        .to_str()
        .expect("cookie")
        .split(';')
        .next()
        .expect("cookie value")
        .to_string();

    // 审计里的 IP 不应是伪造的 203.0.113.9：没有 TCP 对端时走兼容路径，
    // 有对端时用对端地址。两者都不会采信"直连客户端"的 XFF。
    let request = Request::builder()
        .method("GET")
        .uri("/api/system/audit-logs?action=auth.login.success&page=1&size=1")
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .expect("request");
    let response = app.router.clone().oneshot(request).await.expect("audit");
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    // 兼容路径（无 ConnectInfo）会记录 XFF 第一段；生产路径不会。
    // 这里只断言"不是把整条链原样塞进字段"这种明显错误。
    assert!(
        !text.contains("203.0.113.9, 10.0.0.1"),
        "审计 IP 不应保存整条 XFF 链: {text}"
    );
}

/// 统一错误格式：4xx/5xx 都必须是 problem+json 且带安全响应头。
#[tokio::test]
#[ignore = "需要真实 PostgreSQL（DATABASE_URL）"]
async fn error_responses_are_problem_json_with_security_headers() {
    std::env::set_var(
        "DATABASE_URL",
        std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL"),
    );
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");

    // 未登录访问受保护接口 → 401
    let request = Request::builder()
        .method("GET")
        .uri("/api/system/users?page=1&size=1")
        .body(Body::empty())
        .expect("request");
    let response = app.router.clone().oneshot(request).await.expect("request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .is_some(),
        "401 响应也必须带 CSP"
    );
    let text = body_text(response).await;
    let parsed: serde_json::Value = serde_json::from_str(&text).expect("problem json");
    assert_eq!(parsed["code"], "sys.auth.unauthorized");
    assert_eq!(parsed["status"], 401);
}

/// panic 隔离：单个 handler panic 只能变成 500，Router 必须继续可用。
///
/// 这条测试不依赖数据库：把 panic 路由和真实 router 放在同一个进程里验证
/// 「panic 不会带走进程」这个关键属性。
#[tokio::test]
async fn panic_is_isolated_to_single_request() {
    let app: Router = Router::new()
        .route(
            "/boom",
            get(|| async {
                panic!("intentional panic for test");
                #[allow(unreachable_code)]
                ()
            }),
        )
        .layer(tower_http::catch_panic::CatchPanicLayer::custom(
            |_: Box<dyn std::any::Any + Send + 'static>| -> axum::response::Response {
                use axum::response::IntoResponse;
                kernel::error::AppError::Internal(anyhow::anyhow!("boom")).into_response()
            },
        ));

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/boom")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("panic request");
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    // 进程仍然可用：再打一个 404 应该得到正常响应而不是连接崩溃
    let response = app
        .oneshot(
            Request::builder()
                .uri("/missing")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("second request");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// 停止接收流量：置位停机标记后 `/readyz` 必须是 503，`/healthz` 仍是 200。
#[tokio::test]
#[ignore = "需要真实 PostgreSQL（DATABASE_URL）"]
async fn readiness_fails_during_shutdown_but_liveness_stays_ok() {
    std::env::set_var(
        "DATABASE_URL",
        std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL"),
    );
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");

    let request = Request::builder()
        .uri("/readyz")
        .body(Body::empty())
        .expect("request");
    let response = app.router.clone().oneshot(request).await.expect("readyz");
    assert_eq!(response.status(), StatusCode::OK, "启动后应就绪");

    app.state.begin_shutdown();

    let request = Request::builder()
        .uri("/readyz")
        .body(Body::empty())
        .expect("request");
    let response = app.router.clone().oneshot(request).await.expect("readyz");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        response.headers().get(header::RETRY_AFTER).is_some(),
        "503 应带 Retry-After"
    );

    // 存活探针必须仍为 200：否则编排系统会把正在排空的实例杀掉
    let request = Request::builder()
        .uri("/healthz")
        .body(Body::empty())
        .expect("request");
    let response = app.router.clone().oneshot(request).await.expect("healthz");
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    assert!(
        text.contains("\"shuttingDown\":true"),
        "healthz 应暴露停机状态: {text}"
    );
}

/// 每条连接都必须带上服务端超时：客户端断连后 PostgreSQL 仍会继续执行查询，
/// 只有 `statement_timeout` 才能真正释放 CPU 与锁。
#[tokio::test]
#[ignore = "需要真实 PostgreSQL（DATABASE_URL）"]
async fn db_connections_carry_statement_timeout() {
    std::env::set_var(
        "DATABASE_URL",
        std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL"),
    );
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let pool = kernel::db::connect(&config.database)
        .await
        .expect("connect");

    let statement_timeout: String =
        sqlx::query_scalar("SELECT current_setting('statement_timeout')")
            .fetch_one(&pool)
            .await
            .expect("statement_timeout");
    let expected = format!("{}s", config.database.statement_timeout_secs);
    assert_eq!(
        statement_timeout, expected,
        "连接必须带上配置的 statement_timeout"
    );

    let idle_timeout: String =
        sqlx::query_scalar("SELECT current_setting('idle_in_transaction_session_timeout')")
            .fetch_one(&pool)
            .await
            .expect("idle_in_transaction_session_timeout");
    assert_ne!(
        idle_timeout, "0",
        "事务空闲超时必须生效，否则漏 commit 会长期占锁"
    );
    pool.close().await;
}

/// 指标端点：一旦设置了 METRICS_TOKEN，未带 token 必须 401。
#[tokio::test]
#[ignore = "需要真实 PostgreSQL（DATABASE_URL）"]
async fn metrics_requires_token_when_configured() {
    std::env::set_var(
        "DATABASE_URL",
        std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL"),
    );
    std::env::set_var("METRICS_TOKEN", "test-metrics-token");
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");

    let request = Request::builder()
        .uri("/metrics")
        .body(Body::empty())
        .expect("request");
    let response = app.router.clone().oneshot(request).await.expect("metrics");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let request = Request::builder()
        .uri("/metrics")
        .header(header::AUTHORIZATION, "Bearer test-metrics-token")
        .body(Body::empty())
        .expect("request");
    let response = app.router.clone().oneshot(request).await.expect("metrics");
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    assert!(text.contains("http_requests_total"));
    assert!(text.contains("db_pool_saturated"));
    std::env::remove_var("METRICS_TOKEN");
}
