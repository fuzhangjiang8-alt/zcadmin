//! 异步非阻塞流量遥测管道与后台主动探针缓存。

use crate::metrics::{RouteMetricVo, TrafficSummaryVo};
use crate::state::AppState;
use chrono::{TimeZone, Utc};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// 异步遥测事件队列容量（超出时 `try_send` 静默丢弃，保护业务接口）。
const CHANNEL_CAPACITY: usize = 4096;

static TELEMETRY: OnceLock<TelemetryStore> = OnceLock::new();
static SENDER: OnceLock<mpsc::Sender<TelemetryEvent>> = OnceLock::new();
static PROBE_CACHE: OnceLock<RwLock<CachedProbes>> = OnceLock::new();

struct TelemetryEvent {
    method: String,
    path: String,
    status_code: u16,
    elapsed_us: u64,
}

/// 后台异步探针缓存快照。
#[derive(Clone, Default)]
pub struct CachedProbes {
    /// 数据库状态。
    pub db_status: String,
    /// 数据库探测耗时（毫秒）。
    pub db_latency_ms: f64,
    /// 存储目录是否就绪。
    pub storage_ok: bool,
    /// 存储目录探测耗时（毫秒）。
    pub storage_latency_ms: f64,
    /// 上次探测时刻。
    pub updated_at: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct TelemetryStore {
    pub(crate) total_requests: AtomicU64,
    pub(crate) in_flight: AtomicU64,
    pub(crate) status_2xx: AtomicU64,
    pub(crate) status_4xx: AtomicU64,
    pub(crate) status_5xx: AtomicU64,
    pub(crate) total_duration_us: AtomicU64,
    pub(crate) max_duration_us: AtomicU64,
    routes: RwLock<HashMap<(String, String), Arc<RouteEntry>>>,
}

#[derive(Default)]
struct RouteEntry {
    total: AtomicU64,
    ok_2xx: AtomicU64,
    err_4xx: AtomicU64,
    err_5xx: AtomicU64,
    total_duration_us: AtomicU64,
    max_duration_us: AtomicU64,
    last_status: AtomicU16,
    last_seen_secs: AtomicU64,
}

pub(crate) fn store() -> &'static TelemetryStore {
    TELEMETRY.get_or_init(TelemetryStore::default)
}

fn probe_cache() -> &'static RwLock<CachedProbes> {
    PROBE_CACHE.get_or_init(|| RwLock::new(CachedProbes::default()))
}

fn round2(val: f64) -> f64 {
    (val * 100.0).round() / 100.0
}

/// 启动后台异步遥测聚合协程与 10 秒周期的后台主动健康探针。
pub fn spawn_worker(state: AppState) {
    if SENDER.get().is_none() {
        let (tx, mut rx) = mpsc::channel::<TelemetryEvent>(CHANNEL_CAPACITY);
        if SENDER.set(tx).is_ok() {
            tokio::spawn(async move {
                while let Some(ev) = rx.recv().await {
                    apply_event(&ev.method, &ev.path, ev.status_code, ev.elapsed_us);
                }
            });
        }
    }

    tokio::spawn(async move {
        loop {
            run_background_probe(&state).await;
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });
}

/// 非阻塞投递一次 HTTP 请求遥测事件（业务接口热路径调用，永不等待）。
pub fn record_telemetry(method: &str, path: &str, status_code: u16, elapsed_us: u64) {
    if let Some(tx) = SENDER.get() {
        // 后台异步管道已启动：try_send 纯非阻塞投递，队列满则静默丢弃，对业务接口零影响
        let _ = tx.try_send(TelemetryEvent {
            method: method.to_string(),
            path: path.to_string(),
            status_code,
            elapsed_us,
        });
    } else {
        // 单元测试等未启动后台 worker 的场景：直接走非阻塞原子更新
        apply_event(method, path, status_code, elapsed_us);
    }
}

fn apply_event(method: &str, path: &str, status_code: u16, elapsed_us: u64) {
    let s = store();
    s.total_requests.fetch_add(1, Ordering::Relaxed);
    s.total_duration_us.fetch_add(elapsed_us, Ordering::Relaxed);
    s.max_duration_us.fetch_max(elapsed_us, Ordering::Relaxed);
    match status_code {
        200..=299 => {
            s.status_2xx.fetch_add(1, Ordering::Relaxed);
        }
        400..=499 => {
            s.status_4xx.fetch_add(1, Ordering::Relaxed);
        }
        500..=599 => {
            s.status_5xx.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }

    let key = (method.to_string(), path.to_string());
    if let Ok(map) = s.routes.try_read() {
        if let Some(entry) = map.get(&key) {
            update_route_entry(entry, status_code, elapsed_us);
            return;
        }
        if map.len() >= 256 {
            return;
        }
    }
    if let Ok(mut map) = s.routes.try_write() {
        if !map.contains_key(&key) && map.len() >= 256 {
            return;
        }
        let entry = map
            .entry(key)
            .or_insert_with(|| Arc::new(RouteEntry::default()));
        update_route_entry(entry, status_code, elapsed_us);
    }
}

fn update_route_entry(entry: &RouteEntry, status_code: u16, elapsed_us: u64) {
    entry.total.fetch_add(1, Ordering::Relaxed);
    entry
        .total_duration_us
        .fetch_add(elapsed_us, Ordering::Relaxed);
    entry
        .max_duration_us
        .fetch_max(elapsed_us, Ordering::Relaxed);
    entry.last_status.store(status_code, Ordering::Relaxed);
    entry
        .last_seen_secs
        .store(Utc::now().timestamp().max(0) as u64, Ordering::Relaxed);
    let counter = match status_code {
        200..=299 => Some(&entry.ok_2xx),
        400..=499 => Some(&entry.err_4xx),
        500..=599 => Some(&entry.err_5xx),
        _ => None,
    };
    if let Some(c) = counter {
        c.fetch_add(1, Ordering::Relaxed);
    }
}

/// 后台异步执行一次数据库与存储探针并刷新内存快照。
pub async fn run_background_probe(state: &AppState) {
    let db_start = Instant::now();
    let db_ok = tokio::time::timeout(
        Duration::from_millis(800),
        sqlx::query("SELECT 1").execute(&state.db),
    )
    .await
    .is_ok_and(|r| r.is_ok());
    let db_latency_ms = round2(db_start.elapsed().as_secs_f64() * 1000.0);
    let db_status = if !db_ok {
        "unhealthy"
    } else if state.db_saturated() || db_latency_ms > 200.0 {
        "degraded"
    } else {
        "healthy"
    };

    let st_start = Instant::now();
    let storage_ok = tokio::fs::metadata(&state.config.storage.local_dir)
        .await
        .is_ok();
    let storage_latency_ms = round2(st_start.elapsed().as_secs_f64() * 1000.0);

    if let Ok(mut cache) = probe_cache().write() {
        *cache = CachedProbes {
            db_status: db_status.into(),
            db_latency_ms,
            storage_ok,
            storage_latency_ms,
            updated_at: Some(Instant::now()),
        };
    }
}

/// 读取探针内存快照（默认零 IO；若 `force_refresh` 为真且距上次探测 > 2 秒则异步刷新一次）。
pub async fn load_cached_probes(state: &AppState, force_refresh: bool) -> CachedProbes {
    let should_run = probe_cache()
        .read()
        .map(|c| {
            c.updated_at
                .is_none_or(|t| force_refresh && t.elapsed() >= Duration::from_secs(2))
        })
        .unwrap_or(false);
    if should_run {
        run_background_probe(state).await;
    }
    probe_cache().read().map(|c| c.clone()).unwrap_or_default()
}

/// 读取全局流量统计与按请求量排序的路由明细。
pub fn snapshot_telemetry() -> (TrafficSummaryVo, Vec<RouteMetricVo>) {
    let s = store();
    let total = s.total_requests.load(Ordering::Relaxed);
    let in_flight = s.in_flight.load(Ordering::Relaxed);
    let s2 = s.status_2xx.load(Ordering::Relaxed);
    let s4 = s.status_4xx.load(Ordering::Relaxed);
    let s5 = s.status_5xx.load(Ordering::Relaxed);
    let total_us = s.total_duration_us.load(Ordering::Relaxed);
    let max_us = s.max_duration_us.load(Ordering::Relaxed);

    let summary = TrafficSummaryVo {
        total_requests: total,
        in_flight,
        status2xx: s2,
        status4xx: s4,
        status5xx: s5,
        error_rate_percent: if total > 0 {
            round2((s5 as f64 / total as f64) * 100.0)
        } else {
            0.0
        },
        avg_latency_ms: if total > 0 {
            round2((total_us as f64 / total as f64) / 1000.0)
        } else {
            0.0
        },
        max_latency_ms: round2((max_us as f64) / 1000.0),
    };

    let mut routes: Vec<RouteMetricVo> = s
        .routes
        .read()
        .map(|map| {
            map.iter()
                .map(|((method, path), entry)| {
                    let r_total = entry.total.load(Ordering::Relaxed);
                    let r_dur = entry.total_duration_us.load(Ordering::Relaxed);
                    let r_max = entry.max_duration_us.load(Ordering::Relaxed);
                    let seen_secs = entry.last_seen_secs.load(Ordering::Relaxed);
                    RouteMetricVo {
                        method: method.clone(),
                        path: path.clone(),
                        total: r_total,
                        ok2xx: entry.ok_2xx.load(Ordering::Relaxed),
                        err4xx: entry.err_4xx.load(Ordering::Relaxed),
                        err5xx: entry.err_5xx.load(Ordering::Relaxed),
                        avg_ms: if r_total > 0 {
                            round2((r_dur as f64 / r_total as f64) / 1000.0)
                        } else {
                            0.0
                        },
                        max_ms: round2((r_max as f64) / 1000.0),
                        last_status: entry.last_status.load(Ordering::Relaxed),
                        last_seen_at: (seen_secs > 0)
                            .then(|| Utc.timestamp_opt(seen_secs as i64, 0).single())
                            .flatten(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    routes.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.path.cmp(&b.path)));
    (summary, routes)
}
