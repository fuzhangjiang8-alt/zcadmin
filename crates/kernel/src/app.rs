//! 应用装配器（AppBuilder）：建池 → 内核迁移 → 可插拔插件迁移与同步 → 种子数据 → 路由与中间件链 → 后台任务。
//!
//! 关键约束：
//! - 插件迁移失败则整体启动失败（禁止带着未知 schema 状态半残启动）；
//! - 中间件从外到内：`DefaultBodyLimit` → `security::headers` → `client_ip` → `csrf` → `CORS`
//!   → `Compression` → `metrics` → `RequestId` → `Trace` → `CatchPanic` → `LoadShed/ConcurrencyLimit` → `Timeout`。

use crate::config::Config;
use crate::state::AppState;
use crate::{
    auth, configs, db, exports, files, health, notices, plugin, posts, system, user_import,
};
use anyhow::Context;
use axum::http::{header, HeaderName, HeaderValue, Method, StatusCode};
use axum::response::IntoResponse;
use axum::Router;
use sqlx::migrate::Migrator;
use std::time::Duration;
use tower::limit::ConcurrencyLimitLayer;
use tower::load_shed::LoadShedLayer;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::compression::predicate::{NotForContentType, Predicate, SizeAbove};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

/// 内核自身的迁移（`sys` schema）。
static KERNEL_MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// 装配结果：可运行的路由 + 应用状态（状态可用于优雅停机）。
pub struct App {
    /// 已装配好的 axum 路由（状态已注入）。
    ///
    /// 生产入口必须用 `app.router.into_make_service_with_connect_info::<SocketAddr>()`
    /// 交给 `axum::serve`，否则拿不到 TCP 对端地址，可信代理判断会失效。
    pub router: Router,
    /// 应用状态，供 main 做停机清理等操作。
    pub state: AppState,
}

/// 构建应用。
pub async fn build(config: Config) -> anyhow::Result<App> {
    // 指标记录器必须在使用前安装（幂等）
    crate::metrics::install();
    // 1) 数据库
    let pool = db::connect(&config.database).await?;
    let state = AppState::new(pool, config.clone())?;

    // 2) 内核 schema 与迁移
    db::ensure_schema(&state.db, "sys").await?;
    db::run_migrations(&state.db, "sys", &KERNEL_MIGRATOR).await?;

    // 3) 插件：收集 → 按配置过滤（可插拔） → 校验 → 拓扑排序
    let plugins = plugin::load_enabled_plugins(&config.plugins.disabled)?;
    plugin::validate(&plugins)?;
    tracing::info!(
        enabled = plugins.len(),
        disabled = ?config.plugins.disabled,
        "发现并启用插件"
    );

    // 4) 插件迁移
    for p in &plugins {
        let meta = p.meta();
        db::ensure_schema(&state.db, meta.id).await?;
        if let Some(migrator) = p.migrations() {
            db::run_migrations(&state.db, meta.id, migrator).await?;
        }
        // 权限点由插件声明，内核负责落库（幂等）
        system::sync_plugin_permissions(&state.db, meta.id, &p.permissions()).await?;
        system::sync_plugin_menus(&state.db, &p.menus()).await?;
        tracing::info!(plugin = meta.id, version = meta.version, "插件迁移完成");
    }

    // 4.5) 身份域种子数据：首次启动时创建默认租户与管理员
    system::ensure_seed(&state.db, &config.auth).await?;
    // 菜单初始化必须在种子之后（全新库此时才有租户），且幂等
    system::ensure_default_menus(&state.db).await?;
    system::ensure_default_alert_rules(&state.db).await?;

    // 上次进程崩溃/重启时未完成的导出任务，标记为失败，避免永远 running
    exports::recover_stale(&state.db).await;

    // 5) 路由拼装：内核 + 插件
    let router = Router::new()
        .merge(health::routes())
        .merge(auth::routes())
        .nest("/api/system", system::routes())
        .merge(files::routes())
        .merge(notices::routes())
        .merge(configs::routes())
        .merge(exports::routes())
        .merge(user_import::routes())
        .merge(posts::routes())
        .route(
            "/metrics",
            axum::routing::get(crate::metrics::metrics_handler),
        );

    // 契约文档默认开启（内网联调用），公网部署必须用 EXPOSE_API_DOCS=false 关掉：
    // 它会把全部接口与权限点变成攻击者的地图。关闭时**不注册路由**，
    // 返回的是框架原生 404，与"路由不存在"无法区分。
    let mut router = if config.server.expose_api_docs {
        router
            .route(
                "/api/openapi.json",
                axum::routing::get(crate::openapi::serve),
            )
            .route("/docs", axum::routing::get(crate::openapi::docs))
    } else {
        tracing::warn!("接口文档已关闭（EXPOSE_API_DOCS=false）");
        router
    };

    for p in &plugins {
        let meta = p.meta();
        router = router.nest(&format!("/api/{}", meta.id), p.routes());
        tracing::info!(plugin = meta.id, "插件路由已挂载: /api/{}", meta.id);
    }

    // 6) 中间件
    let cors = build_cors(&config)?;
    let max_concurrency = effective_concurrency(config.server.max_concurrency);

    let router = router
        // 最内层：请求超时（超时预算应小于网关层）
        // 显式指定 408，避免上游拿到一个含义不明的错误。
        // 注意：只约束 handler future，不约束响应体传输。
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(config.server.request_timeout_secs),
        ))
        // 过载保护：并发满时立即 503，而不是排队。
        //
        // 三层必须用 ServiceBuilder 组合后再外层套一次：LoadShed 会把错误类型
        // 变成 `BoxError`，如果分开 `.layer()`，Router 会在中间那一步就拒绝编译。
        //
        // 组合顺序（ServiceBuilder 中先加的在外层）：
        // HandleError（把 BoxError 转成 503）→ LoadShed（检测内层是否就绪）
        // → ConcurrencyLimit（信号量）。
        .layer(
            tower::ServiceBuilder::new()
                .layer(axum::error_handling::HandleErrorLayer::new(
                    |error: tower::BoxError| async move {
                        tracing::warn!(%error, "并发上限已满，请求被快速拒绝");
                        crate::error::AppError::Unavailable
                    },
                ))
                .layer(LoadShedLayer::new())
                .layer(ConcurrencyLimitLayer::new(max_concurrency)),
        )
        // panic 隔离：handler 里任何 panic 只影响当前请求（500），进程继续服务。
        // 必须配合 Cargo.toml 的 `panic = "unwind"`，否则这一层永远不会生效。
        .layer(CatchPanicLayer::custom(handle_panic))
        // 访问日志与耗时（默认是 DEBUG，这里显式提到 INFO，否则生产看不到访问日志）
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(
                    tower_http::trace::DefaultMakeSpan::new()
                        .level(tracing::Level::INFO)
                        .include_headers(false),
                )
                .on_request(tower_http::trace::DefaultOnRequest::new().level(tracing::Level::INFO))
                .on_response(
                    tower_http::trace::DefaultOnResponse::new()
                        .level(tracing::Level::INFO)
                        .latency_unit(tower_http::LatencyUnit::Millis),
                ),
        )
        // 把 request-id 回写到响应头，便于前后端对账
        .layer(PropagateRequestIdLayer::x_request_id())
        // 为每个请求生成 request-id
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        // 指标：记录每个请求的耗时与状态码
        .layer(axum::middleware::from_fn(crate::metrics::track))
        // 压缩：只压文本类且大于 1KB 的响应，避免浪费 CPU 压缩小 JSON / 已压缩的 xlsx
        .layer(
            CompressionLayer::new().compress_when(
                SizeAbove::new(1024)
                    .and(NotForContentType::GRPC)
                    .and(NotForContentType::IMAGES),
            ),
        )
        // CORS（预检请求不应该进入鉴权链）
        .layer(cors)
        // CSRF：不安全方法校验来源（在认证前，省一次会话查询）
        .layer(axum::middleware::from_fn(crate::csrf::guard))
        // 解析客户端 IP（可信代理由配置决定）；必须在限流/审计之前
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::client_ip::inject,
        ))
        // 安全响应头：CSP / X-Frame-Options / Referrer-Policy / HSTS
        // 放在最外层：被 CSRF/限流拒绝的 4xx 响应也要带安全头。
        .layer(axum::middleware::from_fn(crate::security::headers))
        // 默认请求体上限 2MB。没有这道闸，攻击者可以用一个 Content-Length
        // 很大的请求把内存打满；上传/导入路由在自己的路由上单独放宽。
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024))
        // 完成状态注入
        .with_state(state.clone());

    tracing::info!(
        max_concurrency,
        request_timeout_secs = config.server.request_timeout_secs,
        trusted_proxies = state.trusted_proxies.len(),
        "中间件链已装配"
    );

    // 前端静态托管：目录存在时启用（单二进制交付形态）
    let mut router = router;
    let web_dist = config.server.web_dist.clone();
    if std::path::Path::new(&web_dist).is_dir() {
        router = router.fallback_service(
            ServeDir::new(&web_dist)
                .not_found_service(ServeFile::new(format!("{web_dist}/index.html"))),
        );
        tracing::info!(dir = %web_dist, "已启用前端静态托管");
    }

    // 启动期先补齐未来 2 个月的审计分区，再统一派生全部后台周期任务（含分布式锁）
    crate::audit::ensure_partitions(&state.db, 2).await?;
    crate::jobs::spawn(state.clone(), &config);

    Ok(App { router, state })
}

/// panic → 500 的转换器。
///
/// 只把 panic 消息写日志（含路由，便于定位），**不**回传给客户端：
/// panic 文本可能包含 SQL、路径或内存内容，属于内部信息。
/// 响应体与 [`crate::error::AppError`] 的 5xx 保持同一种格式，
/// 前端不需要为 panic 单独写分支。
fn handle_panic(err: Box<dyn std::any::Any + Send + 'static>) -> axum::response::Response {
    let detail = if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        (*s).to_string()
    } else {
        "未知 panic".to_string()
    };
    tracing::error!(panic = %detail, "处理器发生 panic，已转换为 500");
    crate::error::AppError::Internal(anyhow::anyhow!("请求处理发生未预期错误")).into_response()
}

/// 计算实际生效的并发上限。
///
/// 配置为 0（未设置）时按 CPU 核数推导：每核 8 个并发，下限 32、上限 1024。
/// 这个值的意义是「保护进程」，不是「追求吞吐」——真正的吞吐上限由连接池决定。
fn effective_concurrency(configured: usize) -> usize {
    if configured > 0 {
        return configured;
    }
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    (cores * 8).clamp(32, 1024)
}

/// 构造 CORS 策略。
///
/// 必须 `allow_credentials(true)`，否则浏览器不会带上会话 Cookie。
/// 注意：开启凭证后**不能**使用 `*` 通配源，必须是显式白名单。
fn build_cors(config: &Config) -> anyhow::Result<CorsLayer> {
    let mut origins = Vec::with_capacity(config.server.cors_origins.len());
    for origin in &config.server.cors_origins {
        let value = origin
            .parse::<HeaderValue>()
            .with_context(|| format!("非法的 CORS 源: {origin}"))?;
        origins.push(value);
    }
    anyhow::ensure!(!origins.is_empty(), "server.cors_origins 不能为空");

    Ok(CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::ACCEPT,
            // 预留：若将来启用双提交 CSRF token（当前用 Origin/Sec-Fetch-Site 校验），
            // 前端必须能带这个头，否则预检会失败。
            HeaderName::from_static("x-csrf-token"),
            HeaderName::from_static("x-request-id"),
        ]))
}
