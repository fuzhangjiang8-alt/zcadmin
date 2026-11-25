//! 配置回归：测试使用注入的环境变量读取器，不修改并行测试共享的进程环境。

use super::Config;

/// 用指定变量覆盖默认配置，并执行与真实启动相同的校验。
fn configured(pairs: &[(&str, &str)]) -> anyhow::Result<Config> {
    let mut config = Config::default();
    config.apply_env_with(|key| {
        Ok(pairs
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| (*value).to_owned()))
    })?;
    config.validate()?;
    Ok(config)
}

#[test]
fn absent_observability_uses_documented_sample_ratio() {
    let defaults = configured(&[]).expect("default config");
    let empty: Config = toml::from_str("").expect("empty TOML");
    assert_eq!(defaults.observability.sample_ratio, 0.1);
    assert_eq!(empty.observability.sample_ratio, 0.1);
}

#[test]
fn invalid_environment_values_fail_without_echoing_input() {
    for key in [
        "MAX_CONCURRENCY",
        "DB_MAX_CONNECTIONS",
        "OTEL_TRACES_SAMPLER_ARG",
        "STATEMENT_TIMEOUT_SECS",
        "AUTH_SINGLE_SESSION",
        "EXPOSE_API_DOCS",
    ] {
        let error = configured(&[(key, "private-invalid-value")])
            .expect_err("invalid value")
            .to_string();
        assert!(error.contains(key), "{error}");
        assert!(!error.contains("private-invalid-value"), "{error}");
    }
}

#[test]
fn capacity_and_timeout_budgets_are_enforced() {
    for pairs in [
        vec![("STATEMENT_TIMEOUT_SECS", "0")],
        vec![("REQUEST_TIMEOUT_SECS", "5")],
        vec![("DB_ACQUIRE_TIMEOUT_SECS", "30")],
        vec![("DB_MAX_CONNECTIONS", "4")],
        vec![("DB_MAX_CONNECTIONS", "0")],
        vec![("SHUTDOWN_GRACE_SECS", "0")],
        vec![("AUTH_SESSION_TTL_SECS", "0")],
        vec![("STORAGE_MAX_FILE_BYTES", "0")],
        vec![("OTEL_TRACES_SAMPLER_ARG", "NaN")],
        vec![("OTEL_TRACES_SAMPLER_ARG", "1.1")],
    ] {
        assert!(configured(&pairs).is_err(), "{pairs:?}");
    }
    assert!(configured(&[("DB_MAX_CONNECTIONS", "2"), ("DB_MIN_CONNECTIONS", "0")]).is_ok());
}

#[test]
fn deployment_overrides_cover_existing_config_fields() {
    let config = configured(&[
        ("AUTH_SINGLE_SESSION", "YES"),
        ("EXPOSE_API_DOCS", "no"),
        ("REQUEST_TIMEOUT_SECS", "40"),
        ("DB_ACQUIRE_TIMEOUT_SECS", "7"),
        ("DB_MIN_CONNECTIONS", "1"),
        ("DB_MAX_LIFETIME_SECS", "600"),
        ("AUTH_SESSION_TTL_SECS", "3600"),
        ("AUTH_MAX_FAILED_ATTEMPTS", "3"),
        ("AUTH_LOCK_SECONDS", "120"),
        ("STORAGE_LOCAL_DIR", "custom/files"),
        ("STORAGE_MAX_FILE_BYTES", "1024"),
        ("AUDIT_RETENTION_DAYS", "365"),
        ("TENANT_RETENTION_DAYS", "60"),
        ("TENANT_BILL_OVERDUE_DAYS", "15"),
        (
            "CORS_ORIGINS",
            " http://localhost:5173, ,http://localhost:4173 ",
        ),
        ("DISABLED_PLUGINS", " workflow, demo "),
        ("OTEL_EXPORTER_OTLP_ENDPOINT", " "),
    ])
    .expect("valid overrides");
    assert!(config.auth.single_session);
    assert!(!config.server.expose_api_docs);
    assert_eq!(config.server.request_timeout_secs, 40);
    assert_eq!(config.database.acquire_timeout_secs, 7);
    assert_eq!(config.database.min_connections, 1);
    assert_eq!(config.database.max_lifetime_secs, 600);
    assert_eq!(config.auth.session_ttl_secs, 3600);
    assert_eq!(config.auth.max_failed_attempts, 3);
    assert_eq!(config.auth.lock_seconds, 120);
    assert_eq!(config.storage.local_dir, "custom/files");
    assert_eq!(config.storage.max_file_bytes, 1024);
    assert_eq!(config.audit.retention_days, 365);
    assert_eq!(config.tenant.retention_days, 60);
    assert_eq!(config.tenant.bill_overdue_days, 15);
    assert_eq!(config.server.cors_origins.len(), 2);
    assert_eq!(config.plugins.disabled, vec!["workflow", "demo"]);
    assert!(config.observability.otlp_endpoint.is_none());
}
