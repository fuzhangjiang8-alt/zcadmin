//! 健康检查端点。
//!
//! 两个探针语义不同，**不要混用**：
//!
//! | 端点 | 含义 | 失败时编排系统应做什么 |
//! |---|---|---|
//! | `/healthz` | 进程还活着（不查依赖） | 重启容器 |
//! | `/readyz` | 依赖就绪，可以接流量 | 从负载均衡摘除，**不要重启** |
//!
//! # 关键约束
//!
//! - `/healthz` 在停机期间**仍然返回 200**：否则编排系统会把正常排空的实例当成故障杀掉；
//! - `/readyz` 在「正在停机」或「数据库不可用」时返回 503：让流量先离开，再关监听；
//! - `/readyz` 的数据库检查必须有超时，否则数据库卡死时探针自己先挂住。
//!
//! 注意：**不要**把「连接池暂时没有空闲连接」当作不就绪。高并发下这属于正常
//! 瞬时状态，把它写进探针会让负载均衡反复摘除/放回实例（抖动），比过载更糟。
//! 池饱和度只通过 `/metrics` 暴露，由告警规则处理。

use crate::error::AppError;
use crate::state::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use std::time::Duration;

/// 就绪探针里 `SELECT 1` 的超时。
///
/// 必须远小于编排系统的 probe timeout（默认 1s），否则探针会被平台先掐断，
/// 表现为"没有响应"而不是明确的 503。
const READY_DB_TIMEOUT: Duration = Duration::from_millis(800);

/// 健康检查路由。
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(liveness))
        .route("/readyz", get(readiness))
}

/// 存活探针：只要能响应就说明进程正常。
async fn liveness(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "uptimeSecs": state.uptime_secs(),
        "shuttingDown": state.is_shutting_down(),
    }))
}

/// 就绪探针：停机中 / 数据库不可用 时返回 503。
async fn readiness(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    if state.is_shutting_down() {
        return Err(AppError::Unavailable);
    }
    let checked =
        tokio::time::timeout(READY_DB_TIMEOUT, sqlx::query("SELECT 1").execute(&state.db)).await;
    match checked {
        Ok(Ok(_)) => Ok(Json(json!({ "status": "ready" }))),
        // 数据库不可用或查询超时：503 让流量离开，而不是 500 让人以为要重启。
        Ok(Err(error)) => {
            tracing::warn!(%error, "就绪检查失败：数据库不可用");
            Err(AppError::Unavailable)
        }
        Err(_) => {
            tracing::warn!(
                "就绪检查超时：数据库响应超过 {}ms",
                READY_DB_TIMEOUT.as_millis()
            );
            Err(AppError::Unavailable)
        }
    }
}
