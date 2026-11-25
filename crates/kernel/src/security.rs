//! 安全响应头。
//!
//! # 为什么需要
//!
//! 浏览器默认行为对管理后台并不安全：允许被 iframe 嵌套（点击劫持）、
//! 允许 MIME 嗅探（把上传文件当脚本执行）、Referer 会把内部 URL 带到外站。
//! 这些不是业务逻辑，但属于「默认安全」的一部分。
//!
//! # 边界
//!
//! - 只加响应头，不改变请求处理与状态码；
//! - CSP 必须兼容 Ant Design Vue 的运行时样式（`style-src 'unsafe-inline'`）；
//! - HSTS 只在 release 构建加：本地 http 开发不该被浏览器记住。
//!
//! # 关键约束
//!
//! 以后接入 CDN / 第三方脚本时，必须先改这里的 CSP，否则会被浏览器拦截。

use axum::body::Body;
use axum::http::{header, HeaderName, HeaderValue, Request};
use axum::middleware::Next;
use axum::response::Response;

/// 逐请求注入安全响应头。
pub async fn headers(req: Request<Body>, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    // frame-ancestors 与 X-Frame-Options 双保险（旧浏览器读前者，新浏览器读后者）
    // connect-src 只放 'self'：源码里没有任何 WebSocket 使用（见 notices.rs 的说明），
    // 放开 `ws:` / `wss:` 等于允许页面把窃取的数据发到任意 WS 端点。
    // 将来真正上 WS 网关时，再显式加回具体域名，不要用通配。
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; \
             img-src 'self' data: blob:; \
             style-src 'self' 'unsafe-inline'; \
             script-src 'self'; \
             font-src 'self' data:; \
             connect-src 'self'; \
             frame-ancestors 'none'; \
             base-uri 'self'; \
             form-action 'self'; \
             object-src 'none'",
        ),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );

    // 生产（HTTPS）才发 HSTS；本地 http 下发会让浏览器强制 https，影响开发。
    if !cfg!(debug_assertions) {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }

    response
}
