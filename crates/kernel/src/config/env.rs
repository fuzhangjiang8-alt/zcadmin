//! 环境变量覆盖：部署容量与安全配置可通过容器环境调整，格式错误只报告变量名。
//! 注入读取器供测试使用，避免修改进程全局环境导致并行测试串扰。

use super::Config;
use std::str::FromStr;

impl Config {
    /// 应用已声明的部署变量；非法值立即失败，不回显可能包含秘密的原始值。
    pub(super) fn apply_env_with(
        &mut self,
        get: impl Fn(&str) -> anyhow::Result<Option<String>>,
    ) -> anyhow::Result<()> {
        override_value(&get, "BIND_ADDR", &mut self.server.bind)?;
        override_value(&get, "WEB_DIST", &mut self.server.web_dist)?;
        override_value(&get, "MAX_CONCURRENCY", &mut self.server.max_concurrency)?;
        override_value(
            &get,
            "REQUEST_TIMEOUT_SECS",
            &mut self.server.request_timeout_secs,
        )?;
        override_value(
            &get,
            "SHUTDOWN_GRACE_SECS",
            &mut self.server.shutdown_grace_secs,
        )?;
        override_bool(&get, "EXPOSE_API_DOCS", &mut self.server.expose_api_docs)?;
        self.apply_database_env(&get)?;
        self.apply_auth_env(&get)?;
        self.apply_storage_env(&get)?;
        self.apply_shared_env(&get)
    }

    /// 数据库连接池和 SQL 超时使用同一套显式解析规则。
    fn apply_database_env(
        &mut self,
        get: &impl Fn(&str) -> anyhow::Result<Option<String>>,
    ) -> anyhow::Result<()> {
        override_value(get, "DATABASE_URL", &mut self.database.url)?;
        override_value(
            get,
            "DB_MAX_CONNECTIONS",
            &mut self.database.max_connections,
        )?;
        override_value(
            get,
            "DB_MIN_CONNECTIONS",
            &mut self.database.min_connections,
        )?;
        override_value(
            get,
            "DB_ACQUIRE_TIMEOUT_SECS",
            &mut self.database.acquire_timeout_secs,
        )?;
        override_value(
            get,
            "DB_MAX_LIFETIME_SECS",
            &mut self.database.max_lifetime_secs,
        )?;
        override_value(
            get,
            "STATEMENT_TIMEOUT_SECS",
            &mut self.database.statement_timeout_secs,
        )?;
        Ok(())
    }

    /// 初始化账号与会话策略可部署覆盖，不回显口令。
    fn apply_auth_env(
        &mut self,
        get: &impl Fn(&str) -> anyhow::Result<Option<String>>,
    ) -> anyhow::Result<()> {
        override_value(get, "AUTH_DEV_USERNAME", &mut self.auth.dev_username)?;
        override_value(get, "AUTH_DEV_PASSWORD", &mut self.auth.dev_password)?;
        override_value(
            get,
            "AUTH_SESSION_TTL_SECS",
            &mut self.auth.session_ttl_secs,
        )?;
        override_value(
            get,
            "AUTH_MAX_FAILED_ATTEMPTS",
            &mut self.auth.max_failed_attempts,
        )?;
        override_value(get, "AUTH_LOCK_SECONDS", &mut self.auth.lock_seconds)?;
        override_bool(get, "AUTH_SINGLE_SESSION", &mut self.auth.single_session)
    }

    /// 文件容量与数据保留期限由部署配置控制，零值约束统一在 validate 校验。
    fn apply_storage_env(
        &mut self,
        get: &impl Fn(&str) -> anyhow::Result<Option<String>>,
    ) -> anyhow::Result<()> {
        override_value(get, "STORAGE_LOCAL_DIR", &mut self.storage.local_dir)?;
        override_value(
            get,
            "STORAGE_MAX_FILE_BYTES",
            &mut self.storage.max_file_bytes,
        )?;
        override_value(get, "AUDIT_RETENTION_DAYS", &mut self.audit.retention_days)?;
        override_value(
            get,
            "TENANT_RETENTION_DAYS",
            &mut self.tenant.retention_days,
        )?;
        override_value(
            get,
            "TENANT_BILL_OVERDUE_DAYS",
            &mut self.tenant.bill_overdue_days,
        )?;
        Ok(())
    }

    /// 列表变量忽略空项，空 OTLP 端点表示关闭远程追踪。
    fn apply_shared_env(
        &mut self,
        get: &impl Fn(&str) -> anyhow::Result<Option<String>>,
    ) -> anyhow::Result<()> {
        override_value(
            get,
            "OTEL_TRACES_SAMPLER_ARG",
            &mut self.observability.sample_ratio,
        )?;
        if let Some(value) = get("OTEL_EXPORTER_OTLP_ENDPOINT")? {
            self.observability.otlp_endpoint = if value.trim().is_empty() {
                None
            } else {
                Some(value)
            };
        }
        for (key, target) in [
            ("CORS_ORIGINS", &mut self.server.cors_origins),
            ("TRUSTED_PROXIES", &mut self.server.trusted_proxies),
            ("DISABLED_PLUGINS", &mut self.plugins.disabled),
        ] {
            if let Some(value) = get(key)? {
                *target = value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
            }
        }
        Ok(())
    }
}

/// 统一解析标量；解析错误不附带原始值，防止把连接串或口令写入日志。
fn override_value<T: FromStr>(
    get: &impl Fn(&str) -> anyhow::Result<Option<String>>,
    key: &str,
    target: &mut T,
) -> anyhow::Result<()> {
    if let Some(value) = get(key)? {
        *target = value
            .parse()
            .map_err(|_| anyhow::anyhow!("环境变量 {key} 格式错误"))?;
    }
    Ok(())
}

/// 保留已有 1/true/yes 写法，并显式拒绝拼错的布尔值，防止关闭安全开关。
fn override_bool(
    get: &impl Fn(&str) -> anyhow::Result<Option<String>>,
    key: &str,
    target: &mut bool,
) -> anyhow::Result<()> {
    if let Some(value) = get(key)? {
        *target = match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => true,
            "0" | "false" | "no" => false,
            _ => anyhow::bail!("环境变量 {key} 必须是 true/false、1/0 或 yes/no"),
        };
    }
    Ok(())
}
