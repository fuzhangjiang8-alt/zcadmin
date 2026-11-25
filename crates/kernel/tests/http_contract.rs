//! HTTP 错误契约回归：使用真实 Router 验证媒体类型、限流状态与重试头，不依赖数据库。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::response::IntoResponse;
use axum::{routing::get, Router};
use kernel::error::AppError;
use tower::ServiceExt;

/// 解析受限响应体，防止测试不小心读取无限流。
async fn problem(response: axum::response::Response) -> serde_json::Value {
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/problem+json"
    );
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("problem JSON")
}

#[tokio::test]
async fn errors_preserve_status_code_and_hide_internal_details() {
    for (error, status, code) in [
        (
            AppError::Unauthorized,
            StatusCode::UNAUTHORIZED,
            "sys.auth.unauthorized",
        ),
        (
            AppError::Forbidden,
            StatusCode::FORBIDDEN,
            "sys.auth.forbidden",
        ),
        (
            AppError::Unavailable,
            StatusCode::SERVICE_UNAVAILABLE,
            "sys.internal.unavailable",
        ),
        (
            AppError::Internal(anyhow::anyhow!("private database connection")),
            StatusCode::INTERNAL_SERVER_ERROR,
            "sys.internal.unexpected",
        ),
        (
            AppError::Database(sqlx::Error::PoolTimedOut),
            StatusCode::SERVICE_UNAVAILABLE,
            "sys.internal.unavailable",
        ),
        (
            AppError::Database(sqlx::Error::PoolClosed),
            StatusCode::SERVICE_UNAVAILABLE,
            "sys.internal.unavailable",
        ),
    ] {
        let response = error.into_response();
        assert_eq!(response.status(), status);
        if status == StatusCode::SERVICE_UNAVAILABLE {
            assert_eq!(response.headers()[header::RETRY_AFTER], "5");
        }
        let body = problem(response).await;
        assert_eq!(body["code"], code);
        assert_eq!(body["status"], status.as_u16());
        assert!(!body.to_string().contains("private database connection"));
    }
}

#[tokio::test]
async fn logout_does_not_claim_success_when_session_revocation_fails() {
    let config = kernel::config::Config::default();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy(&config.database.url)
        .expect("lazy pool");
    pool.close().await;
    let state = kernel::AppState::new(pool, config).expect("state");
    let router = kernel::auth::routes().with_state(state);
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/logout")
        .header(
            header::COOKIE,
            format!("{}=test-token", kernel::auth::SESSION_COOKIE),
        )
        .body(Body::empty())
        .expect("request");
    let response = router.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    assert_eq!(problem(response).await["code"], "sys.internal.unavailable");
}

#[tokio::test]
async fn login_rate_limit_uses_problem_contract() {
    let router = Router::new()
        .route("/login", get(|| async { StatusCode::OK }))
        .layer(axum::middleware::from_fn(kernel::ratelimit::login));
    // 遵守部署覆盖的阈值，测试不修改共享进程环境。
    let limit = std::env::var("LOGIN_RATE_LIMIT_PER_MIN")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(60);
    for _ in 0..limit {
        let request = Request::builder()
            .uri("/login")
            .body(Body::empty())
            .expect("request");
        assert_eq!(
            router
                .clone()
                .oneshot(request)
                .await
                .expect("response")
                .status(),
            StatusCode::OK
        );
    }
    let request = Request::builder()
        .uri("/login")
        .body(Body::empty())
        .expect("request");
    let response = router.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()[header::RETRY_AFTER], "60");
    assert_eq!(problem(response).await["code"], "sys.rate_limit.exceeded");
}
