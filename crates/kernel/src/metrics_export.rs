//! Prometheus 指标端点（渲染 + 访问控制）。
//!
//! # 为什么单独成文件
//!
//! `metrics` 里放的是「指标长什么样」（视图结构）与请求计数中间件；
//! 本文件只负责「怎么把指标端出去、谁能拉」。两者变更原因不同：
//! 前者跟着运维大屏走，后者跟着安全基线走，混在一起会让文件很快超过 300 行。
//!
//! # 访问控制（默认关闭，而不是默认开放）
//!
//! - 配置了 `METRICS_TOKEN`：必须带 `Authorization: Bearer <token>`，常量时间比较；
//! - 未配置：只允许**本机（环回）**与**无网络层信息**的进程内调用（单测 / 嵌入式），
//!   其余请求一律 404 —— 连「这个路由存在」都不外泄。
//!
//! 远程抓取必须显式配置令牌（或做网络隔离），不能把指标端点暴露成
//! 「谁都能拉的路由清单 + 延迟分布 + 连接池水位」。

use crate::state::AppState;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::sync::OnceLock;

/// 全局 Prometheus 句柄。
static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// 初始化指标记录器（进程启动时调用一次）。
pub fn install() {
    if HANDLE.get().is_some() {
        return;
    }
    match PrometheusBuilder::new().install_recorder() {
        Ok(handle) => {
            let _ = HANDLE.set(handle);
            tracing::info!("Prometheus 指标已启用");
        }
        Err(e) => {
            tracing::error!(error = %e, "Prometheus 指标初始化失败");
        }
    }
    // 默认关闭远程抓取：没有令牌时只有本机/进程内能读，运维必须知道这件事。
    if std::env::var("METRICS_TOKEN")
        .map(|v| v.trim().is_empty())
        .unwrap_or(true)
    {
        tracing::warn!(
            "未设置 METRICS_TOKEN：/metrics 仅允许本机访问（远程抓取请配置令牌或做网络隔离）"
        );
    }
}

/// 渲染当前指标快照。
pub fn render() -> String {
    HANDLE.get().map(|h| h.render()).unwrap_or_default()
}

/// `GET /metrics` 处理器（鉴权口径见模块文档）。
pub async fn metrics_handler(
    State(state): State<AppState>,
    client: crate::client_ip::ClientMeta,
    headers: axum::http::HeaderMap,
) -> Response {
    match std::env::var("METRICS_TOKEN")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        Some(expected) => {
            let provided = headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .unwrap_or_default();
            if !constant_time_eq(provided.as_bytes(), expected.as_bytes()) {
                return StatusCode::UNAUTHORIZED.into_response();
            }
        }
        None => {
            if !is_local_or_embedded(&client) {
                tracing::warn!(
                    client_ip = client.ip.as_deref().unwrap_or("-"),
                    "未配置 METRICS_TOKEN，拒绝远程抓取 /metrics"
                );
                return StatusCode::NOT_FOUND.into_response();
            }
        }
    }

    metrics::gauge!("db_pool_connections").set(state.db.size() as f64);
    metrics::gauge!("db_pool_idle").set(state.db.num_idle() as f64);
    metrics::gauge!("db_pool_saturated").set(f64::from(u8::from(state.db_saturated())));

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        render(),
    )
        .into_response()
}

/// 未配置令牌时是否允许访问：本机环回，或没有 TCP 对端（进程内调用）。
fn is_local_or_embedded(client: &crate::client_ip::ClientMeta) -> bool {
    match client.ip.as_deref() {
        None => true,
        Some(ip) => ip
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false),
    }
}

/// 常量时间字节比较（长度不同也走完整循环，避免长度泄漏带来额外信息）。
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut diff = left.len() ^ right.len();
    let max = left.len().max(right.len());
    for index in 0..max {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        diff |= usize::from(a ^ b);
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;
    use crate::client_ip::ClientMeta;

    #[test]
    fn constant_time_eq_matches_semantics() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret-longer"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }

    /// 没有对端信息 = 进程内调用（单测 / oneshot），允许读取；
    /// 非环回地址不允许；环回允许。
    #[test]
    fn access_policy_follows_peer_address() {
        assert!(super::is_local_or_embedded(&ClientMeta::default()));
        assert!(super::is_local_or_embedded(&ClientMeta {
            ip: Some("127.0.0.1".into()),
            ..ClientMeta::default()
        }));
        assert!(super::is_local_or_embedded(&ClientMeta {
            ip: Some("::1".into()),
            ..ClientMeta::default()
        }));
        assert!(!super::is_local_or_embedded(&ClientMeta {
            ip: Some("203.0.113.9".into()),
            ..ClientMeta::default()
        }));
    }
}
