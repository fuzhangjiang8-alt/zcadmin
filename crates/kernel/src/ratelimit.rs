//! 登录接口限流。
//!
//! # 为什么需要
//!
//! 账号锁定（5 次失败锁 15 分钟）只能防止对**单个账号**的暴力破解，
//! 攻击者仍可无限次尝试不同用户名，或反复触发锁定制造拒绝服务。
//! 这里在 IP 维度加一层固定窗口限流，作为第一道闸。
//!
//! # 边界
//!
//! - 只保护 `/api/auth/login`，不影响其他接口；
//! - 进程内计数（多实例各自计数），够用；需要全局精确限流时换 Redis；
//! - 默认 60 次/分钟/IP，可用 `LOGIN_RATE_LIMIT_PER_MIN` 覆盖。
//!
//! # 关键约束
//!
//! 客户端 IP 只能来自 [`crate::client_ip::ClientMeta`]（由 `client_ip::inject`
//! 结合 TCP 对端与可信代理解析）。**禁止**在本模块直接读 `X-Forwarded-For`：
//! 那等于把限流的 key 交给攻击者，换一个伪造头就绕过。
//!
//! 限流本身是进程内计数：多实例时实际阈值 = 单实例阈值 × 实例数。
//! 需要全局精确限流时再引入 Redis（见 `docs/01-后端架构.md`）。

use axum::body::Body;
use axum::http::{header, Request};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 固定窗口长度。
const WINDOW: Duration = Duration::from_secs(60);

/// 默认每分钟每 IP 上限。
const DEFAULT_LIMIT: u32 = 60;

/// 单 IP 计数桶。
struct Bucket {
    window_start: Instant,
    count: u32,
}

/// 全局计数表。
///
/// 为什么用 `Mutex<HashMap>`：限流是极短临界区（几次算术），
/// 引入 DashMap/Redis 只会增加依赖；10k 个 key 时做一次惰性清理。
static BUCKETS: OnceLock<Mutex<HashMap<String, Bucket>>> = OnceLock::new();

/// 判断某个 key 在 `now` 时刻是否放行（纯函数，便于单测）。
fn allow_at(map: &mut HashMap<String, Bucket>, key: &str, now: Instant, limit: u32) -> bool {
    let bucket = map.entry(key.to_string()).or_insert(Bucket {
        window_start: now,
        count: 0,
    });
    if now.duration_since(bucket.window_start) >= WINDOW {
        bucket.window_start = now;
        bucket.count = 0;
    }
    if bucket.count >= limit {
        return false;
    }
    bucket.count += 1;
    true
}

/// 取限流 key：优先用中间件解析好的客户端 IP，缺失时退化为单桶。
///
/// 「缺失」只会出现在中间件顺序被改坏的情况下；此时所有请求共享一个桶，
/// 表现为整体限流而不是绕过——失败方向是安全的。
fn client_ip(req: &Request<Body>) -> String {
    req.extensions()
        .get::<crate::client_ip::ClientMeta>()
        .and_then(|meta| meta.ip.clone())
        .unwrap_or_else(|| "unknown".to_string())
}

/// 按可信客户端 IP 限流；超限返回统一 problem+json 与窗口对应的 Retry-After。
pub async fn login(req: Request<Body>, next: Next) -> Response {
    let limit = std::env::var("LOGIN_RATE_LIMIT_PER_MIN")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_LIMIT);

    let key = client_ip(&req);
    let allowed = {
        let map = BUCKETS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
        if guard.len() > 10_000 {
            guard.retain(|_, b| b.window_start.elapsed() < WINDOW);
        }
        allow_at(&mut guard, &key, Instant::now(), limit)
    };

    if !allowed {
        tracing::warn!(ip = %key, limit, "登录请求被限流");
        let mut response = crate::error::AppError::RateLimited.into_response();
        response.headers_mut().insert(
            header::RETRY_AFTER,
            axum::http::HeaderValue::from_static("60"),
        );
        return response;
    }

    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_within_limit_and_blocks_after() {
        let mut map = HashMap::new();
        let t0 = Instant::now();
        for _ in 0..3 {
            assert!(allow_at(&mut map, "1.2.3.4", t0, 3));
        }
        assert!(!allow_at(&mut map, "1.2.3.4", t0, 3), "第 4 次应被拒绝");
        // 其他 IP 不受影响
        assert!(allow_at(&mut map, "5.6.7.8", t0, 3));
    }

    #[test]
    fn window_resets_after_expiry() {
        let mut map = HashMap::new();
        let t0 = Instant::now();
        assert!(allow_at(&mut map, "ip", t0, 1));
        assert!(!allow_at(&mut map, "ip", t0, 1));
        assert!(allow_at(&mut map, "ip", t0 + WINDOW, 1), "新窗口应放行");
    }
}
