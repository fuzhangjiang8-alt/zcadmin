//! 客户端来源解析（可信代理 + 真实 IP + User-Agent）。
//!
//! 规则：TCP 对端不在 [`TrustedProxies`] 白名单时一律取对端地址并忽略转发头；
//! 在白名单内时从右往左扫描 `X-Forwarded-For` 取第一个不可信地址；无对端（单测/oneshot）退化取首段。
//! 边界：只负责识别来源供限流与审计使用，不做鉴权。

use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap};
use axum::middleware::Next;
use axum::response::Response;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

/// 客户端元信息（IP + User-Agent）。
///
/// 由 [`inject`] 中间件写入请求扩展；handler 里直接声明 `client: ClientMeta` 即可取得。
#[derive(Clone, Debug, Default)]
pub struct ClientMeta {
    /// 解析后的客户端 IP（可信代理场景下是真实客户端，而非代理地址）。
    pub ip: Option<String>,
    /// 原始 `User-Agent`。
    pub user_agent: Option<String>,
}

impl<S> FromRequestParts<S> for ClientMeta
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // 正常情况下中间件已经注入；缺失时（独立路由/测试）按同一规则即时解析。
        Ok(parts
            .extensions
            .get::<ClientMeta>()
            .cloned()
            .unwrap_or_else(|| resolve(None, &TrustedProxies::default(), &parts.headers)))
    }
}

/// 可信反向代理白名单（CIDR）。
///
/// 空列表是安全默认值：不信任任何转发头。部署在 Nginx/网关之后时，
/// 必须通过 `server.trusted_proxies` 或 `TRUSTED_PROXIES` 显式声明代理网段。
#[derive(Clone, Debug, Default)]
pub struct TrustedProxies {
    /// IPv4 网段：(网络地址, 掩码)。
    v4: Vec<(u32, u32)>,
    /// IPv6 网段：(网络地址, 掩码)。
    v6: Vec<(u128, u128)>,
}

impl TrustedProxies {
    /// 解析配置中的 CIDR 列表；非法条目直接报错，避免「配错了但静默不生效」。
    pub fn parse(entries: &[String]) -> anyhow::Result<Self> {
        let mut v4 = Vec::new();
        let mut v6 = Vec::new();
        for raw in entries {
            let text = raw.trim();
            if text.is_empty() {
                continue;
            }
            let (addr, prefix) = match text.split_once('/') {
                Some((addr, prefix)) => {
                    let prefix: u8 = prefix
                        .parse()
                        .map_err(|_| anyhow::anyhow!("非法的可信代理前缀长度: {text}"))?;
                    (addr, prefix)
                }
                None => (text, u8::MAX),
            };
            let ip: IpAddr = addr
                .parse()
                .map_err(|_| anyhow::anyhow!("非法的可信代理地址: {text}"))?;
            match ip {
                IpAddr::V4(v4_addr) => {
                    let prefix = if prefix == u8::MAX { 32 } else { prefix };
                    anyhow::ensure!(prefix <= 32, "IPv4 前缀长度不能超过 32: {text}");
                    let mask = if prefix == 0 {
                        0
                    } else {
                        u32::MAX << (32 - prefix)
                    };
                    v4.push((u32::from(v4_addr) & mask, mask));
                }
                IpAddr::V6(v6_addr) => {
                    let prefix = if prefix == u8::MAX { 128 } else { prefix };
                    anyhow::ensure!(prefix <= 128, "IPv6 前缀长度不能超过 128: {text}");
                    let mask = if prefix == 0 {
                        0
                    } else {
                        u128::MAX << (128 - prefix)
                    };
                    v6.push((u128::from(v6_addr) & mask, mask));
                }
            }
        }
        Ok(Self { v4, v6 })
    }

    /// 地址是否落在白名单网段内。
    pub fn contains(&self, ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(addr) => {
                let value = u32::from(addr);
                self.v4
                    .iter()
                    .any(|(network, mask)| value & mask == *network)
            }
            IpAddr::V6(addr) => {
                let value = u128::from(addr);
                self.v6
                    .iter()
                    .any(|(network, mask)| value & mask == *network)
            }
        }
    }

    /// 白名单中的条目数（用于启动日志）。
    pub fn len(&self) -> usize {
        self.v4.len() + self.v6.len()
    }

    /// 是否为空（未配置代理）。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 中间件：把解析后的 [`ClientMeta`] 注入请求扩展。
///
/// 必须是**最外层**中间件之一：限流、CSRF、审计都依赖它先完成解析。
pub async fn inject(
    State(state): State<crate::state::AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip());
    let meta = resolve(peer, &state.trusted_proxies, req.headers());
    req.extensions_mut().insert(meta);
    next.run(req).await
}

/// 按「对端地址 + 可信代理 + 转发头」推导客户端信息。
fn resolve(peer: Option<IpAddr>, trusted: &TrustedProxies, headers: &HeaderMap) -> ClientMeta {
    let ip = match peer {
        Some(peer) if trusted.contains(peer) => forwarded_ip(headers, trusted)
            .or_else(|| single_ip(headers, "x-real-ip"))
            // 代理没带转发头时退回代理地址：宁可记录代理，也不要丢来源。
            .or_else(|| Some(peer.to_string())),
        // 直连客户端（或未配置代理）：转发头一律不采信。
        Some(peer) => Some(peer.to_string()),
        // 没有网络层信息（测试/嵌入式）：退化为传统读取方式。
        None => forwarded_ip_legacy(headers),
    };
    ClientMeta {
        ip,
        user_agent: headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string),
    }
}

/// 从右往左找第一个不可信的 XFF 地址。
///
/// 代理链 `client, proxy1, proxy2` 中，越靠右越接近本机、越可信；
/// 最左侧可被客户端任意填写，所以只有在右侧全部可信时才可信。
fn forwarded_ip(headers: &HeaderMap, trusted: &TrustedProxies) -> Option<String> {
    let raw = headers.get("x-forwarded-for")?.to_str().ok()?;
    let mut candidate = None;
    for segment in raw.rsplit(',') {
        let text = segment.trim();
        if text.is_empty() {
            continue;
        }
        let Ok(ip) = text.parse::<IpAddr>() else {
            // 非法条目（含端口、域名等）无法判断可信性，停止向更左侧采信。
            break;
        };
        candidate = Some(ip);
        if !trusted.contains(ip) {
            return Some(ip.to_string());
        }
    }
    // 整条链都可信（或全部非法）时，退回到最左侧的合法地址。
    candidate.map(|ip| ip.to_string())
}

/// 无对端地址时的兼容路径：取 XFF 第一段（仅测试/嵌入式场景会走到）。
fn forwarded_ip_legacy(headers: &HeaderMap) -> Option<String> {
    if let Some(first) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(first.to_string());
    }
    single_ip(headers, "x-real-ip")
}

/// 读取单值 IP 头（`X-Real-IP`）。
fn single_ip(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty() && v.parse::<IpAddr>().is_ok())
        .map(str::to_string)
}

/// 解析配置，失败时直接终止启动（由调用方转换为 anyhow 错误）。
pub fn parse_trusted_proxies(entries: &[String]) -> anyhow::Result<Arc<TrustedProxies>> {
    Ok(Arc::new(TrustedProxies::parse(entries)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn proxies(list: &[&str]) -> TrustedProxies {
        let owned: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        TrustedProxies::parse(&owned).expect("parse")
    }

    fn headers_with(name: &'static str, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, HeaderValue::from_str(value).expect("header"));
        headers
    }

    #[test]
    fn cidr_matching_is_correct() {
        let trusted = proxies(&["10.0.0.0/8", "192.168.1.1", "2001:db8::/32"]);
        assert!(trusted.contains("10.1.2.3".parse().expect("ip")));
        assert!(trusted.contains("192.168.1.1".parse().expect("ip")));
        assert!(!trusted.contains("192.168.1.2".parse().expect("ip")));
        assert!(trusted.contains("2001:db8::1".parse().expect("ip")));
        assert!(!trusted.contains("2001:db9::1".parse().expect("ip")));
        assert!(!trusted.contains("203.0.113.9".parse().expect("ip")));
    }

    #[test]
    fn direct_client_cannot_spoof_forwarded_header() {
        let trusted = proxies(&["10.0.0.0/8"]);
        let headers = headers_with("x-forwarded-for", "1.2.3.4");
        let meta = resolve(Some("203.0.113.9".parse().expect("ip")), &trusted, &headers);
        assert_eq!(meta.ip.as_deref(), Some("203.0.113.9"));
    }

    #[test]
    fn trusted_proxy_uses_rightmost_untrusted_hop() {
        let trusted = proxies(&["10.0.0.0/8"]);
        // 客户端伪造了最左侧的 1.1.1.1；真实链路是 203.0.113.9 -> 10.0.0.5
        let headers = headers_with("x-forwarded-for", "1.1.1.1, 203.0.113.9, 10.0.0.5");
        let meta = resolve(Some("10.0.0.5".parse().expect("ip")), &trusted, &headers);
        assert_eq!(meta.ip.as_deref(), Some("203.0.113.9"));
    }

    #[test]
    fn no_peer_falls_back_to_legacy_first_hop() {
        let meta = resolve(
            None,
            &TrustedProxies::default(),
            &headers_with("x-forwarded-for", "203.0.113.9, 10.0.0.1"),
        );
        assert_eq!(meta.ip.as_deref(), Some("203.0.113.9"));
    }

    #[test]
    fn empty_trust_list_ignores_forwarded_header_from_proxy() {
        let headers = headers_with("x-forwarded-for", "203.0.113.9");
        let meta = resolve(
            Some("10.0.0.5".parse().expect("ip")),
            &TrustedProxies::default(),
            &headers,
        );
        assert_eq!(meta.ip.as_deref(), Some("10.0.0.5"));
    }

    #[test]
    fn invalid_cidr_is_rejected() {
        assert!(TrustedProxies::parse(&["10.0.0.0/33".to_string()]).is_err());
        assert!(TrustedProxies::parse(&["not-an-ip".to_string()]).is_err());
    }
}
