//! 配置文件加载与启动校验；只验证现有配置约束，不在这里启动服务或访问数据库。
//! 配置缺失允许使用默认值，配置损坏及非法环境变量必须中止启动。

use super::{Config, ObservabilityConfig};
use anyhow::Context;
use std::path::Path;

impl Config {
    /// 加载 TOML 并应用环境变量；文件不存在使用默认值，解析或校验失败不做降级。
    ///
    /// `path` 是配置路径；返回完整配置，不修改文件、不连接数据库。
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let mut cfg = match std::fs::read_to_string(path) {
            Ok(raw) => toml::from_str::<Config>(&raw)
                .with_context(|| format!("解析配置文件失败: {}", path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(path = %path.display(), "配置文件不存在，使用默认配置 + 环境变量");
                Self::default()
            }
            Err(error) => {
                return Err(error).with_context(|| format!("读取配置文件失败: {}", path.display()))
            }
        };
        cfg.apply_env_with(|key| match std::env::var(key) {
            Ok(value) => Ok(Some(value)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => {
                anyhow::bail!("环境变量 {key} 必须是 UTF-8 文本")
            }
        })?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// 把关闭 SQL 超时、无效容量和反向超时预算挡在启动期，避免线上无限等待。
    pub(super) fn validate(&self) -> anyhow::Result<()> {
        self.validate_database()?;
        self.validate_runtime()
    }

    /// 请求必须覆盖获取连接与语句执行的各层超时，零值不能意外关闭保护。
    fn validate_database(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.database.url.trim().is_empty(),
            "DATABASE_URL 不能为空"
        );
        anyhow::ensure!(
            self.database.max_connections > 0,
            "database.max_connections 必须大于 0"
        );
        anyhow::ensure!(
            self.database.min_connections <= self.database.max_connections,
            "database.min_connections 不能大于 max_connections"
        );
        for (name, seconds) in [
            (
                "database.acquire_timeout_secs",
                self.database.acquire_timeout_secs,
            ),
            (
                "database.statement_timeout_secs",
                self.database.statement_timeout_secs,
            ),
        ] {
            anyhow::ensure!(
                seconds > 0 && seconds < self.server.request_timeout_secs,
                "{name} 必须大于 0 且小于 server.request_timeout_secs（超时预算要逐层递减）"
            );
        }
        anyhow::ensure!(
            self.database.max_lifetime_secs > 0,
            "database.max_lifetime_secs 必须大于 0"
        );
        Ok(())
    }

    /// 校验身份、文件和维护任务参数；这些值不能依赖运行中异常来兜底。
    fn validate_runtime(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.server.shutdown_grace_secs > 0,
            "server.shutdown_grace_secs 必须大于 0"
        );
        anyhow::ensure!(
            self.auth.session_ttl_secs > 0,
            "auth.session_ttl_secs 必须大于 0"
        );
        anyhow::ensure!(
            self.auth.max_failed_attempts > 0 && self.auth.lock_seconds > 0,
            "auth.max_failed_attempts 与 auth.lock_seconds 必须大于 0"
        );
        anyhow::ensure!(
            !self.auth.dev_username.trim().is_empty() && !self.auth.dev_password.is_empty(),
            "初始管理员用户名和密码不能为空"
        );
        anyhow::ensure!(
            !self.storage.local_dir.trim().is_empty() && self.storage.max_file_bytes > 0,
            "storage.local_dir 不能为空且 max_file_bytes 必须大于 0"
        );
        anyhow::ensure!(
            self.audit.retention_days > 0 && self.tenant.retention_days > 0,
            "审计及租户保留天数必须大于 0"
        );
        anyhow::ensure!(
            (0.0..=1.0).contains(&self.observability.sample_ratio),
            "observability.sample_ratio 必须在 0.0~1.0 之间"
        );
        Ok(())
    }
}

/// TOML 缺省字段与整个配置缺省必须使用相同采样率。
pub(super) fn default_sample_ratio() -> f64 {
    0.1
}

impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            otlp_endpoint: None,
            sample_ratio: default_sample_ratio(),
        }
    }
}
