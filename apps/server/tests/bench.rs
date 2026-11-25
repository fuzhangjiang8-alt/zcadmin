//! 性能基线（真库 + 完整中间件链，不走 TCP）。
//!
//! 为什么用 `oneshot` 而不是 HTTP 压测工具：
//! - 本机/CI 的防火墙可能拦截本地端口；
//! - 这里要测的是「应用 + 数据库」的处理能力，不是网卡。
//!
//! 运行：
//! ```powershell
//! $env:DATABASE_URL="postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin"
//! cargo test -p server --test bench -- --ignored --nocapture
//! ```
//!
//! 输出：请求数、并发、RPS、p50/p95/p99（毫秒）、错误数。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;
use tower::ServiceExt;

// 强制链接插件（与 main.rs 同理），否则 /api/crm 路由不存在
use plugin_crm as _;
use plugin_demo as _;

const REQUESTS: usize = 400;
const CONCURRENCY: usize = 25;
const SEED_ROWS: usize = 100;

async fn login(router: &axum::Router) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "username": "admin",
                "password": std::env::var("AUTH_DEV_PASSWORD").unwrap_or_else(|_| "admin123456".into()),
                "tenant": null
            })
            .to_string(),
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

async fn one_get(router: &axum::Router, uri: &str, cookie: &str) -> Result<(), StatusCode> {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.expect("request");
    let status = resp.status();
    // 必须读完 body，否则测不到序列化成本
    let _ = axum::body::to_bytes(resp.into_body(), 8 * 1024 * 1024)
        .await
        .expect("body");
    if status == StatusCode::OK {
        Ok(())
    } else {
        Err(status)
    }
}

fn percentile(sorted: &[u128], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx] as f64 / 1000.0
}

#[tokio::test]
#[ignore = "需要真实 PostgreSQL（DATABASE_URL）"]
async fn bench_crm_list_and_dashboard() {
    let url = std::env::var("DATABASE_URL").expect("必须设置 DATABASE_URL");
    std::env::set_var("DATABASE_URL", &url);
    let config = kernel::config::Config::load("config/default.toml").expect("config");
    let app = kernel::app::build(config).await.expect("build app");
    let cookie = login(&app.router).await;

    // 造一点数据，避免列表只返回 1 行
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    for i in 0..SEED_ROWS {
        let body = json!({
            "code": format!("BENCH-{suffix}-{i}"),
            "name": format!("Bench Customer {i}"),
            "level": "normal"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/crm/customers")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::COOKIE, &cookie)
            .body(Body::from(body.to_string()))
            .unwrap();
        let _ = app.router.clone().oneshot(req).await;
    }

    let mut uris = vec![
        "/api/crm/customers?page=1&size=20".to_string(),
        "/api/crm/dashboard/summary".to_string(),
    ];
    // 详情类接口需要一条已存在的合同；由 scripts/seed-bench.sql 提供
    if let Ok(contract_id) = std::env::var("BENCH_CONTRACT_ID") {
        uris.push(format!("/api/crm/contracts/{contract_id}/payments"));
    }

    for uri in uris {
        // 预热
        for _ in 0..20 {
            let _ = one_get(&app.router, &uri, &cookie).await;
        }

        let semaphore = Arc::new(tokio::sync::Semaphore::new(CONCURRENCY));
        let started = Instant::now();
        let mut handles = Vec::with_capacity(REQUESTS);
        for _ in 0..REQUESTS {
            let router = app.router.clone();
            let cookie = cookie.clone();
            let sem = semaphore.clone();
            let uri = uri.clone();
            handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                let t0 = Instant::now();
                let result = one_get(&router, &uri, &cookie).await;
                (t0.elapsed().as_micros(), result)
            }));
        }

        let mut latencies = Vec::with_capacity(REQUESTS);
        let mut errors = 0usize;
        for h in handles {
            let (lat, result) = h.await.expect("join");
            latencies.push(lat);
            if result.is_err() {
                errors += 1;
            }
        }
        let elapsed = started.elapsed().as_secs_f64();
        latencies.sort_unstable();

        println!(
            "BENCH {uri}: n={REQUESTS} concurrency={CONCURRENCY} rps={:.1} p50={:.1}ms p95={:.1}ms p99={:.1}ms errors={errors}",
            REQUESTS as f64 / elapsed,
            percentile(&latencies, 0.50),
            percentile(&latencies, 0.95),
            percentile(&latencies, 0.99)
        );
        assert_eq!(errors, 0, "bench had {errors} errors");
    }
}
