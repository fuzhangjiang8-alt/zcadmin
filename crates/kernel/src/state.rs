//! 应用状态（组合根）。
//!
//! Rust 没有运行时依赖注入容器。我们采用**显式组合根**：
//! 所有共享依赖都在 [`AppState`] 里以字段形式声明，启动时一次构造，之后 `Clone`（内部全是 `Arc`）传给每个 handler。
//!
//! 这样做的好处：
//! - 依赖关系在编译期可见，没有"运行时找不到 bean"的问题；
//! - AI 改代码时能一眼看到"这个服务有哪些依赖"；
//! - 测试时可以只构造需要的部分。
//!
//! # 停机状态
//!
//! [`AppState::shutting_down`] 是进程级的「正在排空」标记：
//! 收到 SIGTERM 后立即置位，`/readyz` 随即将 503，负载均衡停止转发新流量，
//! 而 in-flight 请求继续处理。没有这个标记，滚动发布会把请求打到正在退出的实例上。

use crate::auth::SessionStore;
use crate::client_ip::TrustedProxies;
use crate::config::Config;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 全局应用状态。
///
/// `Clone` 很便宜（内部都是 `Arc` / 连接池句柄），axum 会在每个请求里克隆一份。
#[derive(Clone)]
pub struct AppState {
    /// PostgreSQL 连接池。
    pub db: PgPool,
    /// 只读配置（启动后不再修改）。
    pub config: Arc<Config>,
    /// 会话存储。
    ///
    /// 生产实现为 PostgreSQL 表 `sys.sys_session`，多实例共享；令牌仅存 SHA-256 摘要。
    pub sessions: SessionStore,
    /// 对象存储（M3 为本地磁盘实现）。
    pub storage: crate::storage::LocalStorage,
    /// 可信反向代理网段；决定是否采信 `X-Forwarded-For`。
    pub trusted_proxies: Arc<TrustedProxies>,
    /// 进程启动时间，用于 `/healthz` 返回运行时长。
    pub boot_time: DateTime<Utc>,
    /// 是否正在优雅停机（置位后 `/readyz` 返回 503）。
    shutting_down: Arc<AtomicBool>,
}

impl AppState {
    /// 构造应用状态。
    ///
    /// 失败语义：可信代理 CIDR 配置非法时返回错误，由启动流程终止进程
    /// （配置错误必须在启动期暴露，不能静默降级成"不信任任何代理"）。
    pub fn new(db: PgPool, config: Config) -> anyhow::Result<Self> {
        let storage = crate::storage::LocalStorage::new(&config.storage.local_dir);
        let sessions = SessionStore::new(db.clone());
        let trusted_proxies =
            crate::client_ip::parse_trusted_proxies(&config.server.trusted_proxies)
                .map_err(|e| anyhow::anyhow!("server.trusted_proxies 配置非法: {e}"))?;
        Ok(Self {
            db,
            config: Arc::new(config),
            sessions,
            storage,
            trusted_proxies,
            boot_time: Utc::now(),
            shutting_down: Arc::new(AtomicBool::new(false)),
        })
    }

    /// 进程已运行秒数。
    pub fn uptime_secs(&self) -> i64 {
        (Utc::now() - self.boot_time).num_seconds()
    }

    /// 进入停机流程：`/readyz` 立即返回 503，新流量应被负载均衡摘除。
    pub fn begin_shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
    }

    /// 是否正在停机。
    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }

    /// 连接池是否已经饱和（无空闲连接且达到上限）。
    ///
    /// **只用于指标与告警**，不要用于 `/readyz`：高并发下"瞬时无空闲连接"
    /// 是正常状态，写进探针会导致负载均衡反复摘除实例。
    pub fn db_saturated(&self) -> bool {
        self.db.num_idle() == 0 && self.db.size() >= self.config.database.max_connections
    }
}
