//! 配置加载（优先级：代码默认值 < `config/default.toml` < 环境变量覆盖）。
//!
//! 边界：本文件声明配置结构与默认值；读取与校验在 `loading`，环境变量覆盖在 `env`。

use serde::Deserialize;

/// 应用总配置。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// HTTP 服务相关配置。
    #[serde(default)]
    pub server: ServerConfig,
    /// 数据库相关配置。
    #[serde(default)]
    pub database: DatabaseConfig,
    /// 认证相关配置。
    #[serde(default)]
    pub auth: AuthConfig,
    /// 对象存储相关配置。
    #[serde(default)]
    pub storage: StorageConfig,
    /// 可观测性配置。
    #[serde(default)]
    pub observability: ObservabilityConfig,
    /// 审计日志保留策略。
    #[serde(default)]
    pub audit: AuditConfig,
    /// 租户清理策略。
    #[serde(default)]
    pub tenant: TenantConfig,
    /// 插件启停配置（可插拔控制）。
    #[serde(default)]
    pub plugins: PluginsConfig,
}

/// 插件启停配置（运行期可插拔）。
///
/// 内部系统不需要某些可选业务模块时，可通过 `config/default.toml` 的 `[plugins]`
/// 或环境变量 `DISABLED_PLUGINS=workflow,crm` 将其拔除（跳过迁移、权限、菜单与路由挂载）。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PluginsConfig {
    /// 禁用的插件 id 列表（如 `["workflow", "demo"]`）。
    #[serde(default)]
    pub disabled: Vec<String>,
}

/// 租户清理配置。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenantConfig {
    /// 软删除租户保留天数，默认 30 天。
    #[serde(default = "default_tenant_retention_days")]
    pub retention_days: u64,
    /// 账单逾期宽限天数，默认 30 天。
    #[serde(default = "default_bill_overdue_days")]
    pub bill_overdue_days: u64,
}

fn default_bill_overdue_days() -> u64 {
    30
}

fn default_tenant_retention_days() -> u64 {
    30
}

impl Default for TenantConfig {
    fn default() -> Self {
        Self {
            retention_days: default_tenant_retention_days(),
            bill_overdue_days: default_bill_overdue_days(),
        }
    }
}

/// 审计日志保留配置。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditConfig {
    /// 保留天数，默认 180 天（等保三级要求至少 6 个月）。
    #[serde(default = "default_retention_days")]
    pub retention_days: u64,
}

fn default_retention_days() -> u64 {
    180
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            retention_days: default_retention_days(),
        }
    }
}

/// 可观测性配置。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservabilityConfig {
    /// OTLP 端点（如 http://127.0.0.1:4317）。为空则只输出本地日志。
    pub otlp_endpoint: Option<String>,
    /// 链路采样比例（0.0~1.0）。默认 0.1。
    ///
    /// 全量采样在 1000 QPS 下会自己把 exporter 压垮，还会把追踪后端打满。
    /// 排障时可以临时用 `OTEL_TRACES_SAMPLER_ARG` 调到 1.0。
    #[serde(default = "default_sample_ratio")]
    pub sample_ratio: f64,
}

use loading::default_sample_ratio;

/// 对象存储配置。
///
/// M3 只实现本地磁盘后端；接入 S3/MinIO 时新增 `kind` 字段并替换实现，
/// 业务代码不需要改动（统一走 `LocalStorage` 的抽象接口）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageConfig {
    /// 本地存储根目录（相对或绝对路径）。
    pub local_dir: String,
    /// 单文件大小上限（字节）。
    pub max_file_bytes: u64,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            local_dir: "data/files".to_string(),
            max_file_bytes: 20 * 1024 * 1024,
        }
    }
}

/// HTTP 服务配置。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// 监听地址，例如 `0.0.0.0:8080`。
    pub bind: String,
    /// 允许跨域的前端源（开发环境是 Vite 的 5173 端口）。
    pub cors_origins: Vec<String>,
    /// 单次请求超时（秒）。必须小于网关超时，形成逐层递减的超时预算。
    pub request_timeout_secs: u64,
    /// 同时处理中的请求上限（0 表示按 CPU 核数推导，超过立即 503）。
    #[serde(default)]
    pub max_concurrency: usize,
    /// 可信反向代理的 CIDR 列表（为空表示不信任任何代理，客户端 IP 取 TCP 对端）。
    #[serde(default)]
    pub trusted_proxies: Vec<String>,
    /// 优雅停机的排空上限（秒，须小于编排系统 `stop_grace_period`）。
    #[serde(default = "default_shutdown_grace_secs")]
    pub shutdown_grace_secs: u64,
    /// 是否对外暴露 `/api/openapi.json` 与 `/docs`（公网部署应设为 `false`）。
    #[serde(default = "default_expose_api_docs")]
    pub expose_api_docs: bool,
    /// 前端构建产物目录。目录存在时由后端直接托管 SPA（单二进制交付）。
    #[serde(default = "default_web_dist")]
    pub web_dist: String,
}

fn default_shutdown_grace_secs() -> u64 {
    20
}

fn default_expose_api_docs() -> bool {
    true
}

/// 数据库配置。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    /// PostgreSQL 连接串，例如 `postgres://user:pass@localhost:5432/db`。
    pub url: String,
    /// 连接池最大连接数。经验公式见 docs/03-数据架构.md 第 7 节。
    pub max_connections: u32,
    /// 从池中获取连接的超时（秒）。超时返回 503 而不是无限排队。
    pub acquire_timeout_secs: u64,
    /// 单条 SQL 的服务端超时（秒），默认 5。
    ///
    /// 通过连接级 `statement_timeout` 下发：客户端断开后 PostgreSQL 仍会继续
    /// 执行查询，只有服务端超时才能真正释放资源。必须小于请求总超时。
    #[serde(default = "default_statement_timeout_secs")]
    pub statement_timeout_secs: u64,
    /// 连接最大存活时长（秒），默认 1800。避免长期连接让故障切换/证书轮换失效。
    #[serde(default = "default_max_lifetime_secs")]
    pub max_lifetime_secs: u64,
    /// 预热的空闲连接数，默认 5。启动后立即建连，避免首个请求撞上 TLS/网络握手。
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
}

fn default_statement_timeout_secs() -> u64 {
    5
}

fn default_max_lifetime_secs() -> u64 {
    1800
}

fn default_min_connections() -> u32 {
    5
}

/// 认证配置。
///
/// 账号字段只用于空库初始化；已有账号的认证使用数据库用户表和 Argon2id。
/// 修改初始化口令不会覆盖已存在的密码，会话与锁定字段用于默认策略。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    /// 空库初始化的管理员用户名。
    pub dev_username: String,
    /// 空库初始化密码。生产环境必须用 `AUTH_DEV_PASSWORD` 环境变量覆盖。
    pub dev_password: String,
    /// 会话有效期（秒），默认 8 小时。
    pub session_ttl_secs: i64,
    /// 连续登录失败多少次后锁定账号。
    #[serde(default = "default_max_failed")]
    pub max_failed_attempts: u32,
    /// 锁定时长（秒），默认 15 分钟。
    #[serde(default = "default_lock_seconds")]
    pub lock_seconds: i64,
    /// 是否单点登录：同一账号只保留最新会话（默认 false）。
    #[serde(default)]
    pub single_session: bool,
}

fn default_max_failed() -> u32 {
    5
}

fn default_lock_seconds() -> i64 {
    15 * 60
}

fn default_web_dist() -> String {
    "frontend/dist".to_string()
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0:8080".to_string(),
            cors_origins: vec!["http://localhost:5173".to_string()],
            request_timeout_secs: 30,
            max_concurrency: 0,
            trusted_proxies: Vec::new(),
            shutdown_grace_secs: default_shutdown_grace_secs(),
            expose_api_docs: default_expose_api_docs(),
            web_dist: default_web_dist(),
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            // 与 docker-compose.yml 中的默认账号保持一致，开箱即用。
            url: "postgres://zcadmin:zcadmin@localhost:5432/zcadmin".to_string(),
            max_connections: 20,
            acquire_timeout_secs: 5,
            statement_timeout_secs: default_statement_timeout_secs(),
            max_lifetime_secs: default_max_lifetime_secs(),
            min_connections: default_min_connections(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            dev_username: "admin".to_string(),
            dev_password: "admin123456".to_string(),
            session_ttl_secs: 8 * 3600,
            max_failed_attempts: default_max_failed(),
            lock_seconds: default_lock_seconds(),
            single_session: false,
        }
    }
}

mod env;
mod loading;
#[cfg(test)]
mod tests;
