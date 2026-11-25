//! 服务入口（只做配置、日志追踪、应用装配、监听与优雅停机，不含业务逻辑）。
//!
//! 启动顺序：特殊模式（`--healthcheck` / `--dump-openapi`）→ 配置加载 → 日志与 OTel 初始化
//! → `kernel::app::build` 装配 → 端口监听 → SIGTERM/Ctrl+C 优雅停机（摘流 → 排空 → 关池 → flush OTel）。

use anyhow::Context;
use kernel::config::Config;
use std::net::SocketAddr;
use std::time::Duration;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

// 强制链接已启用 feature 的插件 crate（编译期可插拔）。
#[cfg(feature = "plugin-crm")]
use plugin_crm as _;
#[cfg(feature = "plugin-demo")]
use plugin_demo as _;
#[cfg(feature = "plugin-workflow")]
use plugin_workflow as _;

const DEFAULT_CONFIG_PATH: &str = "config/default.toml";
const DEFAULT_LOG_FILTER: &str = "info";
const DRAIN_GRACE: Duration = Duration::from_secs(3);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().any(|arg| arg == "--healthcheck") {
        return healthcheck().await;
    }
    if std::env::args().any(|arg| arg == "--dump-openapi") {
        return dump_openapi();
    }

    let config_path =
        std::env::var("CONFIG_PATH").unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());
    let config = match Config::load(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("❌ 配置加载失败（{config_path}）: {error:#}");
            return Err(error);
        }
    };

    let otel_guard = init_tracing(&config);
    tracing::info!(
        path = %config_path,
        log_format = std::env::var("LOG_FORMAT").unwrap_or_else(|_| "pretty".into()),
        otel = otel_guard.is_some(),
        "配置加载完成，日志与链路追踪已初始化"
    );

    let app = kernel::app::build(config.clone())
        .await
        .context("应用装配失败")?;

    let listener = tokio::net::TcpListener::bind(&config.server.bind)
        .await
        .with_context(|| format!("端口绑定失败: {}", config.server.bind))?;
    let local_addr = listener
        .local_addr()
        .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));

    tracing::info!(
        addr = %local_addr,
        version = env!("CARGO_PKG_VERSION"),
        shutdown_grace_secs = config.server.shutdown_grace_secs,
        "服务已启动 → http://{}/healthz",
        local_addr
    );

    let grace = Duration::from_secs(config.server.shutdown_grace_secs);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let shutdown_state = app.state.clone();

    // 必须用 into_make_service_with_connect_info：可信代理判断依赖 TCP 对端地址。
    let server = axum::serve(
        listener,
        app.router
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = shutdown_rx.await;
        shutdown_state.begin_shutdown();
        tokio::time::sleep(DRAIN_GRACE).await;
    });

    let serve_result = tokio::select! {
        biased;
        result = server => result,
        _ = drain_watchdog(grace, shutdown_tx) => {
            tracing::warn!(grace_secs = grace.as_secs(), "停机排空超时，放弃剩余请求并强制退出");
            Ok(())
        }
    };

    if tokio::time::timeout(grace, app.state.db.close())
        .await
        .is_err()
    {
        tracing::warn!("连接池关闭超时（未完成的导出任务会在下次启动时回收）");
    }
    if let Some(guard) = otel_guard {
        guard.shutdown();
    }

    serve_result.context("HTTP 服务异常退出")?;
    tracing::info!("服务已优雅退出");
    Ok(())
}

/// 停机 watchdog：等信号 → 通知 server 摘流 → 到点仍未结束则返回。
///
/// 返回即代表"排空超时"，由 `main` 放弃剩余 in-flight 请求。
async fn drain_watchdog(grace: Duration, shutdown_tx: tokio::sync::oneshot::Sender<()>) {
    shutdown_signal().await;
    // 通知 axum 停止接收新连接（发送失败说明 server 已自行退出，无需再等）。
    let _ = shutdown_tx.send(());
    // 留出摘流缓冲后，最多再等 grace 秒让 in-flight 请求收尾。
    tokio::time::sleep(DRAIN_GRACE + grace).await;
}

/// 初始化日志与链路追踪（OTel 失败时仅降级为本地日志，不阻断启动）。
fn init_tracing(config: &Config) -> Option<OtelGuard> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
    let json = std::env::var("LOG_FORMAT").as_deref() == Ok("json");

    if let Some(provider) = build_provider(config) {
        use opentelemetry::trace::TracerProvider as _;
        let otel_layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("zcadmin"));
        let registry = tracing_subscriber::registry().with(filter).with(otel_layer);
        if json {
            registry
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        } else {
            registry
                .with(tracing_subscriber::fmt::layer().pretty())
                .init();
        }
        Some(OtelGuard(provider))
    } else {
        let registry = tracing_subscriber::registry().with(filter);
        if json {
            registry
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        } else {
            registry
                .with(tracing_subscriber::fmt::layer().pretty())
                .init();
        }
        None
    }
}

/// 按配置构建 OTLP provider（未配置端点或初始化失败时返回 `None`）。
fn build_provider(config: &Config) -> Option<opentelemetry_sdk::trace::SdkTracerProvider> {
    let endpoint = config
        .observability
        .otlp_endpoint
        .as_deref()
        .map(str::trim)
        .filter(|endpoint| !endpoint.is_empty())?;
    match init_otel(endpoint, config.observability.sample_ratio) {
        Ok(provider) => Some(provider),
        Err(error) => {
            eprintln!("OTel 初始化失败（仅本地日志）: {error:#}");
            None
        }
    }
}

/// OTel provider 的停机守卫。
///
/// 为什么必须手动持有：batch exporter 有后台线程，进程直接返回会丢掉
/// 最后一批 span（正好是排障时最需要的"退出前发生了什么"）。
struct OtelGuard(opentelemetry_sdk::trace::SdkTracerProvider);

impl OtelGuard {
    fn shutdown(self) {
        if let Err(error) = self.0.shutdown() {
            eprintln!("OTel flush 失败（已尽力发送）: {error}");
        }
    }
}

/// 初始化 OTLP exporter，返回 provider（由调用方持有并在退出时 shutdown）。
///
/// 使用 batch exporter：单条 span 不阻塞请求，由后台线程批量发送。
/// 采样率默认 10%：1000 QPS 全量采样会把追踪后端和 exporter 自己压垮。
///
/// 必须用 `with_sampler` 显式设置：SDK 默认是 `AlwaysOn`（100%），
/// 与 `docs/01-后端架构.md` 的「正常请求 1%」完全相反。
fn init_otel(
    endpoint: &str,
    sample_ratio: f64,
) -> anyhow::Result<opentelemetry_sdk::trace::SdkTracerProvider> {
    use opentelemetry_otlp::WithExportConfig as _;

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .context("创建 OTLP exporter 失败")?;

    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_sampler(opentelemetry_sdk::trace::Sampler::ParentBased(Box::new(
            opentelemetry_sdk::trace::Sampler::TraceIdRatioBased(sample_ratio.clamp(0.0, 1.0)),
        )))
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name("zcadmin")
                .build(),
        )
        .build();

    opentelemetry::global::set_tracer_provider(provider.clone());
    Ok(provider)
}

/// 等待退出信号。
///
/// 支持 Ctrl+C（开发）与 SIGTERM（容器编排）。
/// 收到信号后由 `main` 触发「置 not-ready → 摘流缓冲 → axum 停止接收并排空」。
async fn shutdown_signal() {
    let ctrl_c = async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {}
            Err(error) => {
                tracing::error!(%error, "注册 Ctrl+C 处理器失败，改为等待 SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "注册 SIGTERM 处理器失败，改为等待 Ctrl+C");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("收到 Ctrl+C，开始优雅停机"),
        _ = terminate => tracing::info!("收到 SIGTERM，开始优雅停机"),
    }
}

/// 容器健康检查模式：只做 TCP 连通性探测。
///
/// 为什么不用 `/readyz`：容器内没有 curl，而且探针不应依赖数据库——
/// 数据库故障时容器被反复重启只会让问题更糟。就绪判断交给负载均衡。
async fn healthcheck() -> anyhow::Result<()> {
    let addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    // 监听地址可能是 `[::]:8080` 或 `0.0.0.0:8080`，探测时统一改成回环地址。
    let target = addr
        .replace("[::]", "127.0.0.1")
        .replace("0.0.0.0", "127.0.0.1");
    match tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::TcpStream::connect(&target),
    )
    .await
    {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(error)) => {
            eprintln!("healthcheck failed: {error}");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("healthcheck failed: 连接 {target} 超时");
            std::process::exit(1);
        }
    }
}

/// 导出 OpenAPI 契约到 stdout（不连库、不启服务）。
fn dump_openapi() -> anyhow::Result<()> {
    let plugins = kernel::plugin::load_plugins()?;
    let doc = kernel::openapi::build_with(&plugins);
    println!("{}", serde_json::to_string_pretty(&doc)?);
    Ok(())
}
