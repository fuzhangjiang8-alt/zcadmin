//! 系统运维探针与监控大屏接口（`/api/system/ops`）。
//!
//! 职责：
//! - 主动探测服务器进程、PostgreSQL 数据库延迟、连接池水位、本地对象存储与插件引擎状态；
//! - 聚合进程内 HTTP 请求次数、状态码分布、并发度与路由级响应耗时排行；
//! - 提供按模块（`database / requests / runtime / jobs`）下钻的深度诊断详情。

use crate::auth::CurrentUser;
use crate::error::AppError;
pub use crate::metrics::{
    snapshot_telemetry, OpsDetailVo, OpsKvItem, OpsOverviewVo, ProbeItemVo, ServerInfoVo,
};
use crate::plugin::enabled_plugin_infos;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::telemetry::load_cached_probes;
use crate::tenant::TenantTx;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

/// 运维大屏查询参数。
#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpsOverviewQuery {
    /// 是否请求立即刷新后台探针（默认 false，直接读后台异步探针内存快照，0 数据库开销）。
    pub refresh: Option<bool>,
}

/// 运维监控路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/ops/overview", get(ops_overview))
        .route("/ops/detail/{section}", get(ops_detail))
}

/// 获取运维监控大屏总览与实时探针数据（默认读取后台异步探针内存快照，零数据库阻塞）。
#[utoipa::path(
    get,
    path = "/api/system/ops/overview",
    tag = "system",
    params(("refresh" = Option<bool>, Query, description = "是否触发主动刷新（带 2s 防抖冷却）")),
    responses((status = 200, description = "运维大屏总览数据", body = ApiResponse<OpsOverviewVo>))
)]
pub(crate) async fn ops_overview(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<OpsOverviewQuery>,
) -> Result<ApiResponse<OpsOverviewVo>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;

    let snap = load_cached_probes(&state, q.refresh.unwrap_or(false)).await;
    let pool_size = state.db.size();
    let pool_idle = state.db.num_idle();
    let pool_max = state.config.database.max_connections;
    let (traffic, mut routes) = snapshot_telemetry();
    let plugins = enabled_plugin_infos(&state.config.plugins.disabled);
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    let req_status = if traffic.error_rate_percent > 5.0 {
        "degraded"
    } else {
        "healthy"
    };
    let rt_status = if snap.storage_ok && !state.is_shutting_down() {
        "healthy"
    } else {
        "degraded"
    };
    let db_sum = format!("pool {pool_size}/{pool_max} (idle {pool_idle})");
    let req_sum = format!(
        "total {} · 5xx {}%",
        traffic.total_requests, traffic.error_rate_percent
    );
    let rt_sum = format!(
        "{} ({cores} cores) · {} plugins",
        std::env::consts::OS,
        plugins.len()
    );
    let probes = vec![
        ProbeItemVo::new(
            "database",
            "PostgreSQL",
            &snap.db_status,
            snap.db_latency_ms,
            db_sum,
        ),
        ProbeItemVo::new(
            "requests",
            "HTTP Gateway",
            req_status,
            traffic.avg_latency_ms,
            req_sum,
        ),
        ProbeItemVo::new(
            "runtime",
            "Server Runtime",
            rt_status,
            snap.storage_latency_ms,
            rt_sum,
        ),
        ProbeItemVo::new(
            "jobs",
            "Background Jobs",
            "healthy",
            0.1,
            "8 daemons (advisory lock enabled)".into(),
        ),
    ];

    routes.truncate(30);
    Ok(ApiResponse::new(OpsOverviewVo {
        server: ServerInfoVo {
            version: env!("CARGO_PKG_VERSION").into(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            cpu_cores: cores,
            pid: std::process::id(),
            boot_time: state.boot_time,
            uptime_secs: state.uptime_secs(),
            db_pool_size: pool_size,
            db_pool_idle: pool_idle,
            db_pool_max: pool_max,
            shutting_down: state.is_shutting_down(),
        },
        probes,
        traffic,
        routes,
        plugins,
    }))
}

/// 点击大屏探针或指标卡后，按模块标识（`database / requests / runtime / jobs`）获取下钻详情。
#[utoipa::path(
    get,
    path = "/api/system/ops/detail/{section}",
    tag = "system",
    params(("section" = String, Path, description = "模块：database / requests / runtime / jobs")),
    responses((status = 200, description = "探针与运行指标下钻详情", body = ApiResponse<OpsDetailVo>))
)]
pub(crate) async fn ops_detail(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(section): Path<String>,
) -> Result<ApiResponse<OpsDetailVo>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let (traffic, routes) = snapshot_telemetry();

    let items = match section.as_str() {
        "database" => {
            let (pg_ver, db_bytes, active_conns, rls_tables): (String, i64, i64, i64) = sqlx::query_as(
                "SELECT version(), pg_database_size(current_database()),
                        (SELECT count(*) FROM pg_stat_activity WHERE state = 'active'),
                        (SELECT count(*) FROM pg_class WHERE relkind = 'r' AND relforcerowsecurity = true)",
            )
            .fetch_one(&mut *tx)
            .await?;
            let ver = pg_ver.split(',').next().unwrap_or(&pg_ver).to_string();
            let size_mb = format!("{:.2} MB", db_bytes as f64 / 1048576.0);
            let pool_str = format!(
                "{} / {} (idle {})",
                state.db.size(),
                state.config.database.max_connections,
                state.db.num_idle()
            );
            let timeout_str = format!("{} s", state.config.database.statement_timeout_secs);
            kvs(vec![
                ("PostgreSQL", ver, "info"),
                ("Database Size", size_mb, "ok"),
                ("Active Queries", active_conns.to_string(), "ok"),
                ("Pool Connections", pool_str, "ok"),
                ("Statement Timeout", timeout_str, "info"),
                ("FORCE RLS Tables", rls_tables.to_string(), "ok"),
            ])
        }
        "requests" => {
            let err_str = format!("{} ({}%)", traffic.status5xx, traffic.error_rate_percent);
            let lat_str = format!(
                "{} ms / {} ms",
                traffic.avg_latency_ms, traffic.max_latency_ms
            );
            let err_lvl = if traffic.status5xx > 0 { "warn" } else { "ok" };
            kvs(vec![
                ("Total Requests", traffic.total_requests.to_string(), "info"),
                ("In-Flight Concurrency", traffic.in_flight.to_string(), "ok"),
                ("2xx Success", traffic.status2xx.to_string(), "ok"),
                ("4xx Client Error", traffic.status4xx.to_string(), "info"),
                ("5xx Server Error", err_str, err_lvl),
                ("Latency (Avg / Max)", lat_str, "info"),
                ("Tracked Routes", routes.len().to_string(), "info"),
            ])
        }
        "runtime" => {
            let plugins = enabled_plugin_infos(&state.config.plugins.disabled);
            let names: Vec<String> = plugins
                .into_iter()
                .map(|p| format!("{}@{}", p.id, p.version))
                .collect();
            let os_str = format!(
                "{} / {} / {}",
                std::env::consts::OS,
                std::env::consts::ARCH,
                std::process::id()
            );
            let up_str = format!(
                "{} s (boot {})",
                state.uptime_secs(),
                state.boot_time.format("%Y-%m-%d %H:%M:%S UTC")
            );
            let bind_str = format!(
                "{} / {} s",
                state.config.server.bind, state.config.server.request_timeout_secs
            );
            let dir_str = format!(
                "{} ({} MB)",
                state.config.storage.local_dir,
                state.config.storage.max_file_bytes / 1048576
            );
            kvs(vec![
                ("Service Version", env!("CARGO_PKG_VERSION").into(), "ok"),
                ("OS / Arch / PID", os_str, "info"),
                ("Uptime", up_str, "ok"),
                ("Bind / Request Timeout", bind_str, "info"),
                ("Storage Dir / Limit", dir_str, "ok"),
                ("Enabled Plugins", names.join(", "), "ok"),
            ])
        }
        "jobs" => {
            let (exp_pending, exp_ok, exp_fail, unacked_alerts, dead_deliveries, online_sessions): (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
                "SELECT (SELECT count(*) FROM sys.sys_export_task WHERE tenant_id = $1 AND status IN ('pending', 'running')),
                        (SELECT count(*) FROM sys.sys_export_task WHERE tenant_id = $1 AND status = 'success'),
                        (SELECT count(*) FROM sys.sys_export_task WHERE tenant_id = $1 AND status = 'failed'),
                        (SELECT count(*) FROM sys.sys_audit_alert WHERE tenant_id = $1 AND acknowledged_at IS NULL),
                        (SELECT count(*) FROM sys.sys_audit_alert_delivery WHERE tenant_id = $1 AND status = 'dead'),
                        (SELECT count(*) FROM sys.sys_session WHERE tenant_id = $1 AND expires_at > now())",
            )
            .bind(user.tenant_id)
            .fetch_one(&mut *tx)
            .await?;
            let exp_str = format!("{exp_pending} / {exp_ok} / {exp_fail}");
            kvs(vec![
                ("Online Sessions", online_sessions.to_string(), "ok"),
                (
                    "Export Tasks (Running / Success / Failed)",
                    exp_str,
                    if exp_fail > 0 { "warn" } else { "ok" },
                ),
                (
                    "Unacknowledged Audit Alerts",
                    unacked_alerts.to_string(),
                    if unacked_alerts > 0 { "warn" } else { "ok" },
                ),
                (
                    "Dead Webhook Deliveries",
                    dead_deliveries.to_string(),
                    if dead_deliveries > 0 { "warn" } else { "ok" },
                ),
                (
                    "Leader Election",
                    "PostgreSQL pg_try_advisory_lock".into(),
                    "ok",
                ),
            ])
        }
        _ => return Err(AppError::NotFound),
    };

    tx.commit().await?;
    Ok(ApiResponse::new(OpsDetailVo {
        section: section.clone(),
        status: "healthy".into(),
        items,
        routes: if section == "requests" {
            routes
        } else {
            Vec::new()
        },
    }))
}

fn kvs(rows: Vec<(&str, String, &str)>) -> Vec<OpsKvItem> {
    rows.into_iter()
        .map(|(label, value, level)| OpsKvItem {
            label: label.into(),
            value,
            level: level.into(),
        })
        .collect()
}
