//! Prometheus 指标与进程内实时流量遥测（`path` 使用 axum 路由模板以控制标签基数）。

pub use crate::metrics_export::{install, metrics_handler, render};
use crate::plugin::PluginInfoVo;
pub use crate::telemetry::{record_telemetry, snapshot_telemetry};
use axum::extract::{MatchedPath, Request};
use axum::middleware::Next;
use axum::response::Response;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::sync::atomic::Ordering;

/// 路由级请求统计视图（供运维大屏展示）。
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RouteMetricVo {
    /// HTTP 方法。
    pub method: String,
    /// 路由模板路径。
    pub path: String,
    /// 累计请求次数。
    pub total: u64,
    /// 2xx 次数。
    pub ok2xx: u64,
    /// 4xx 次数。
    pub err4xx: u64,
    /// 5xx 次数。
    pub err5xx: u64,
    /// 平均耗时（毫秒）。
    pub avg_ms: f64,
    /// 最大耗时（毫秒）。
    pub max_ms: f64,
    /// 最近一次 HTTP 状态码。
    pub last_status: u16,
    /// 最近一次访问时间。
    pub last_seen_at: Option<DateTime<Utc>>,
}

/// 全局流量统计快照。
#[derive(Debug, Clone, Default, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TrafficSummaryVo {
    /// 累计请求总次数。
    pub total_requests: u64,
    /// 当前正在处理的并发请求数。
    pub in_flight: u64,
    /// 2xx 成功请求数。
    pub status2xx: u64,
    /// 4xx 客户端错误数。
    pub status4xx: u64,
    /// 5xx 服务端错误数。
    pub status5xx: u64,
    /// 5xx 错误率（0~100 百分比）。
    pub error_rate_percent: f64,
    /// 全局平均响应耗时（毫秒）。
    pub avg_latency_ms: f64,
    /// 全局峰值响应耗时（毫秒）。
    pub max_latency_ms: f64,
}

/// 服务器与运行时基础指标。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfoVo {
    /// 服务版本号。
    pub version: String,
    /// 操作系统。
    pub os: String,
    /// CPU 架构。
    pub arch: String,
    /// 逻辑 CPU 核数。
    pub cpu_cores: usize,
    /// 进程 PID。
    pub pid: u32,
    /// 启动时间（UTC）。
    pub boot_time: DateTime<Utc>,
    /// 已运行秒数。
    pub uptime_secs: i64,
    /// 数据库连接池当前连接数。
    pub db_pool_size: u32,
    /// 数据库连接池空闲连接数。
    pub db_pool_idle: usize,
    /// 数据库连接池最大连接数。
    pub db_pool_max: u32,
    /// 是否正在优雅停机。
    pub shutting_down: bool,
}

/// 单项探针检测结果（点击可按 `key` 下钻详情）。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProbeItemVo {
    /// 探针标识（`database` / `runtime` / `requests` / `jobs`）。
    pub key: String,
    /// 探针名称。
    pub name: String,
    /// 状态：`healthy` / `degraded` / `unhealthy`。
    pub status: String,
    /// 探测耗时（毫秒）。
    pub latency_ms: f64,
    /// 核心摘要描述。
    pub summary: String,
}

impl ProbeItemVo {
    /// 构造探针结果项。
    pub fn new(key: &str, name: &str, status: &str, latency_ms: f64, summary: String) -> Self {
        Self {
            key: key.into(),
            name: name.into(),
            status: status.into(),
            latency_ms,
            summary,
        }
    }
}

/// 运维监控大屏总览响应。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpsOverviewVo {
    /// 服务器与连接池状态。
    pub server: ServerInfoVo,
    /// 主动健康探针列表。
    pub probes: Vec<ProbeItemVo>,
    /// 全局 HTTP 请求与延迟汇总。
    pub traffic: TrafficSummaryVo,
    /// 路由级请求次数与耗时排行（Top 30）。
    pub routes: Vec<RouteMetricVo>,
    /// 已启用插件列表。
    pub plugins: Vec<PluginInfoVo>,
}

/// 下钻详情键值项。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpsKvItem {
    /// 指标名。
    pub label: String,
    /// 指标值。
    pub value: String,
    /// 状态标签（`ok` / `warn` / `info`）。
    pub level: String,
}

/// 运维大屏点击下钻详情视图。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpsDetailVo {
    /// 下钻模块标识（`database` / `requests` / `runtime` / `jobs`）。
    pub section: String,
    /// 当前模块状态。
    pub status: String,
    /// 诊断键值明细列表。
    pub items: Vec<OpsKvItem>,
    /// 关联的路由性能明细（当 `section == "requests"` 时返回全量路由）。
    pub routes: Vec<RouteMetricVo>,
}

/// 请求计数与延迟中间件。
pub async fn track(req: Request, next: Next) -> Response {
    let method = req.method().as_str().to_string();
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "unmatched".to_string());

    let s = crate::telemetry::store();
    s.in_flight.fetch_add(1, Ordering::Relaxed);
    let started = std::time::Instant::now();
    let response = next.run(req).await;
    let elapsed_dur = started.elapsed();
    s.in_flight.fetch_sub(1, Ordering::Relaxed);

    let elapsed = elapsed_dur.as_secs_f64();
    let elapsed_us = elapsed_dur.as_micros() as u64;
    let status_code = response.status().as_u16();
    let status = status_code.to_string();

    record_telemetry(&method, &path, status_code, elapsed_us);

    metrics::counter!(
        "http_requests_total",
        "method" => method.clone(),
        "path" => path.clone(),
        "status" => status
    )
    .increment(1);

    metrics::histogram!(
        "http_request_duration_seconds",
        "method" => method,
        "path" => path
    )
    .record(elapsed);

    response
}

#[cfg(test)]
mod tests {
    use super::{record_telemetry, snapshot_telemetry};

    #[test]
    fn telemetry_records_requests_and_route_metrics() {
        record_telemetry("GET", "/api/system/ops/overview", 200, 1500);
        record_telemetry("GET", "/api/system/ops/overview", 500, 3500);
        let (summary, routes) = snapshot_telemetry();
        assert!(summary.total_requests >= 2);
        let route = routes
            .iter()
            .find(|r| r.path == "/api/system/ops/overview")
            .expect("route");
        assert!(route.total >= 2);
        assert!(route.ok2xx >= 1);
        assert!(route.err5xx >= 1);
    }
}
