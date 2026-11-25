//! CSRF 防护（纵深防御）。
//!
//! # 为什么还需要
//!
//! 会话 Cookie 已是 `SameSite=Lax`，跨站表单 POST 默认不会携带 Cookie。
//! 但 Lax 仍允许顶层 GET 导航，且未来若改成 `SameSite=None` 会立刻失去保护；
//! 多一层来源校验成本很低，能覆盖「Cookie 策略被改坏」的情况。
//!
//! # 策略
//!
//! - `Sec-Fetch-Site: cross-site` → 403（现代浏览器对不安全方法必带）；
//! - 有 `Origin` 且与 `Host`/`X-Forwarded-Host` 不同源 → 403（覆盖老浏览器）；
//!   代理终止 TLS 时用 `X-Forwarded-Proto` 还原真实 scheme，协议不一致同样拒绝；
//! - 两者都没有（curl、服务端调用、集成测试）→ 放行，由认证与权限点负责。
//!
//! # 边界
//!
//! 只拦截 `POST/PUT/PATCH/DELETE`；不替代鉴权，也不处理 CORS 预检（`OPTIONS`）。

use axum::extract::Request;
use axum::http::{header, Method};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// 中间件入口：校验不安全方法的请求来源。
pub async fn guard(req: Request, next: Next) -> Response {
    if is_unsafe(req.method()) && !origin_allowed(req.headers()) {
        return crate::error::AppError::Forbidden.into_response();
    }
    next.run(req).await
}

/// 是否为需要 CSRF 校验的方法。
fn is_unsafe(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

/// 来源是否可信。
fn origin_allowed(headers: &header::HeaderMap) -> bool {
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        if site.eq_ignore_ascii_case("cross-site") {
            return false;
        }
    }
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        // 非浏览器客户端不带 Origin；是否可信由会话与权限决定。
        return true;
    };
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok());
    // 反向代理终止 TLS 时，只有代理才知道原始协议；取不到就不比协议（不能凭空假设 https）。
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    match host {
        Some(host) => same_origin(origin, scheme.as_deref(), host),
        // 没有 Host（HTTP/1.0 或异常代理）时保守拒绝带 Origin 的写请求。
        None => false,
    }
}

/// 比较 `Origin` 与请求的「协议 + authority」是否同源。
///
/// 只比 authority 会放过协议降级（`https://a.example.com` 的页面提交到
/// `http://a.example.com`）；`scheme` 为 `None` 表示拿不到真实协议，
/// 此时只比 authority。
fn same_origin(origin: &str, scheme: Option<&str>, host: &str) -> bool {
    origin
        .split_once("://")
        .map(|(origin_scheme, authority)| {
            let authority_ok = authority.eq_ignore_ascii_case(host);
            match scheme {
                Some(expected) => authority_ok && origin_scheme.eq_ignore_ascii_case(expected),
                None => authority_ok,
            }
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn same_origin_matches_scheme_and_host() {
        assert!(same_origin(
            "https://admin.example.com",
            None,
            "admin.example.com"
        ));
        assert!(same_origin("http://127.0.0.1:8080", None, "127.0.0.1:8080"));
        assert!(!same_origin(
            "https://evil.example.com",
            None,
            "admin.example.com"
        ));
        assert!(!same_origin(
            "https://admin.example.com.evil.com",
            None,
            "admin.example.com"
        ));
        assert!(!same_origin("null", None, "admin.example.com"));
    }

    /// 拿得到真实协议时，协议不一致也要拒绝（防 https 页面被降级提交）。
    #[test]
    fn same_origin_rejects_scheme_downgrade() {
        assert!(!same_origin(
            "http://admin.example.com",
            Some("https"),
            "admin.example.com"
        ));
        assert!(same_origin(
            "https://admin.example.com",
            Some("https"),
            "admin.example.com"
        ));
        assert!(same_origin(
            "https://admin.example.com",
            Some("HTTPS"),
            "admin.example.com"
        ));
    }

    #[test]
    fn unsafe_methods_are_guarded() {
        assert!(is_unsafe(&Method::POST));
        assert!(is_unsafe(&Method::PUT));
        assert!(is_unsafe(&Method::PATCH));
        assert!(is_unsafe(&Method::DELETE));
        assert!(!is_unsafe(&Method::GET));
        assert!(!is_unsafe(&Method::OPTIONS));
    }

    #[test]
    fn cross_site_header_is_rejected() {
        let mut headers = header::HeaderMap::new();
        headers.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        assert!(!origin_allowed(&headers));
    }

    #[test]
    fn missing_origin_is_allowed_for_non_browser_clients() {
        let headers = header::HeaderMap::new();
        assert!(origin_allowed(&headers));
    }

    #[test]
    fn same_origin_header_is_allowed() {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://a.example.com"),
        );
        headers.insert(header::HOST, HeaderValue::from_static("a.example.com"));
        assert!(origin_allowed(&headers));
    }

    #[test]
    fn foreign_origin_is_rejected() {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example.com"),
        );
        headers.insert(header::HOST, HeaderValue::from_static("a.example.com"));
        assert!(!origin_allowed(&headers));
    }
}
