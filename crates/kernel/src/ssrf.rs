//! 出站 HTTP 请求的 SSRF（服务端请求伪造）安全防护。
//!
//! # 职责与边界
//!
//! 任何由租户管理员或用户配置的回调地址（如审计告警 Webhook、订阅 Webhook）
//! 在保存与发起请求前必须经过本模块校验，防止攻击者探测宿主机环回端口、
//! 容器内网（RFC1918 / IPv6 ULA）或云厂商实例元数据服务（`169.254.169.254`）。
//!
//! 两档校验：
//! - [`validate_webhook_url`]：只查 URL 字面量（协议 / 主机名 / 字面量 IP）；
//! - [`validate_webhook_url_async`]：在字面量校验之后**再做一次真实域名解析**，
//!   逐个检查解析结果。凡是要真正发请求的路径都必须用这一档，否则
//!   `http://attacker.example.com → 10.0.0.5` 这类「解析到内网」直接绕过。
//!
//! 残余风险：解析与建连之间仍可能被重解析（TOCTOU），生产应叠加出口网络 ACL。

use std::net::IpAddr;

/// 校验出站 Webhook URL 的协议与目标主机，拦截 SSRF 高危地址（见 `docs/05` §2.1）。
///
/// # 规则
/// - 必须为 `http://` 或 `https://` 且包含合法主机名；
/// - 始终拒绝链路本地/云厂商元数据段（`169.254.0.0/16`、`fe80::/10`）、未指定地址（`0.0.0.0`、`::`）、
///   多播/广播地址以及常见云元数据域名（`metadata.google.internal`）；
/// - 当 `block_private` 为 `true`（生产构建默认开启）时，额外拒绝环回（`127.0.0.0/8`、`::1`、`localhost`）
///   与私网段（`10/8`、`172.16/12`、`192.168/16`、`fc00::/7`、`*.local`、`*.internal`）。
pub fn validate_webhook_url_with(url: &str, block_private: bool) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url.trim())
        .map_err(|_| "Webhook 地址格式非法，必须是合法的 http(s) URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Webhook 必须是 http(s) 地址".into());
    }
    let host = parsed
        .host_str()
        .map(|h| {
            h.trim_matches(|c| c == '[' || c == ']')
                .to_ascii_lowercase()
        })
        .filter(|h| !h.is_empty())
        .ok_or_else(|| "Webhook 地址缺少主机名".to_string())?;

    if host == "metadata.google.internal" || host == "metadata" {
        return Err("Webhook 禁止指向云元数据服务地址".into());
    }
    if block_private
        && (host == "localhost"
            || host.ends_with(".localhost")
            || host.ends_with(".local")
            || host.ends_with(".internal"))
    {
        return Err("生产环境 Webhook 禁止指向本地或内网主机名".into());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_always_forbidden_ip(ip) {
            return Err("Webhook 禁止指向链路本地、未指定或保留地址".into());
        }
        if block_private && is_private_or_loopback_ip(ip) {
            return Err("生产环境 Webhook 禁止指向环回或私网 IP".into());
        }
    }
    Ok(())
}

/// 校验 Webhook URL（生产环境默认拦截私网与环回地址，调试环境放行 127.0.0.1 供回环集成测试）。
pub fn validate_webhook_url(url: &str) -> Result<(), String> {
    validate_webhook_url_with(url, !cfg!(debug_assertions))
}

/// 域名解析超时：保存配置的接口不该被一个坏域名拖住。
const DNS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// 带域名解析的异步校验。
///
/// # 为什么必须做
///
/// [`validate_webhook_url`] 只能看到 URL 里的**字面量**主机名：
/// `http://attacker.example.com` 完全可以解析到 `10.0.0.5` 或 `127.0.0.1`，
/// 只查字面量的实现会直接放行（DNS rebinding 与「解析到内网的公网域名」同理）。
/// 因此凡是要真正发起请求的地方，都应当用本函数。
///
/// # 残余风险
///
/// 校验通过到实际建连之间域名仍可能被重新解析（TOCTOU）。要彻底消除，
/// 需要「按已校验的 IP 建连」或出口网络 ACL；不要把本函数当成唯一防线。
pub async fn validate_webhook_url_async(url: &str) -> Result<(), String> {
    validate_webhook_url(url)?;
    let (host, port) = split_host_port(url)?;
    // 字面量 IP 已在同步校验里判过，无需解析（也避免把 IP 当域名去查 DNS）。
    if host.parse::<IpAddr>().is_ok() {
        return Ok(());
    }

    let block_private = !cfg!(debug_assertions);
    let addrs = tokio::time::timeout(DNS_TIMEOUT, tokio::net::lookup_host((host.as_str(), port)))
        .await
        .map_err(|_| "Webhook 域名解析超时".to_string())?
        .map_err(|_| "Webhook 域名无法解析".to_string())?;

    let mut checked = 0usize;
    for addr in addrs {
        checked += 1;
        let ip = addr.ip();
        if is_always_forbidden_ip(ip) {
            return Err("Webhook 域名解析到链路本地/保留地址，已拒绝".into());
        }
        if block_private && is_private_or_loopback_ip(ip) {
            return Err("生产环境 Webhook 域名解析到环回或私网地址，已拒绝".into());
        }
    }
    if checked == 0 {
        return Err("Webhook 域名没有解析到任何地址".into());
    }
    Ok(())
}

/// 取出小写主机名与端口（用于 DNS 校验）。
fn split_host_port(url: &str) -> Result<(String, u16), String> {
    let parsed = reqwest::Url::parse(url.trim())
        .map_err(|_| "Webhook 地址格式非法，必须是合法的 http(s) URL".to_string())?;
    let host = parsed
        .host_str()
        .map(|h| {
            h.trim_matches(|c| c == '[' || c == ']')
                .trim_end_matches('.')
                .to_ascii_lowercase()
        })
        .filter(|h| !h.is_empty())
        .ok_or_else(|| "Webhook 地址缺少主机名".to_string())?;
    let port = parsed.port_or_known_default().unwrap_or(80);
    Ok((host, port))
}

fn is_always_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_unspecified()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.octets()[0] == 0
        }
        IpAddr::V6(v6) => {
            // IPv4 映射地址（::ffff:a.b.c.d）按 IPv4 规则判，否则 169.254.x.x 会被漏掉。
            if let Some(v4) = v6.to_ipv4() {
                return is_always_forbidden_ip(IpAddr::V4(v4));
            }
            // 环回地址不在这里判：它与 IPv4 的 127/8 一样，只在 block_private 时才拦，
            // 保证 debug 构建仍能对 [::1] 做回环集成测试。
            v6.is_unspecified() || v6.is_multicast() || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

fn is_private_or_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || is_reserved_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4() {
                return is_private_or_loopback_ip(IpAddr::V4(v4));
            }
            v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

/// 非公网可达的保留段（RFC 6890 里常见的几块）。
///
/// `is_private()` 只覆盖 10/8、172.16/12、192.168/16；
/// CGNAT（运营商 NAT，云上常被复用）、文档/基准测试段、240/4 保留段同样是内网可达地址。
fn is_reserved_ipv4(v4: std::net::Ipv4Addr) -> bool {
    let o = v4.octets();
    (o[0] == 100 && (64..=127).contains(&o[1]))   // 100.64.0.0/10 CGNAT
        || (o[0] == 192 && o[1] == 0)              // 192.0.0.0/24 + 192.0.2.0/24
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19)) // 198.18.0.0/15 基准测试
        || (o[0] == 198 && o[1] == 51 && o[2] == 100)  // 198.51.100.0/24 文档
        || (o[0] == 203 && o[1] == 0 && o[2] == 113)   // 203.0.113.0/24 文档
        || o[0] >= 240 // 240.0.0.0/4 保留 + 广播
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_ssrf_metadata_and_private_targets() {
        assert!(
            validate_webhook_url_with("http://169.254.169.254/latest/meta-data", false).is_err()
        );
        assert!(validate_webhook_url_with("http://0.0.0.0:8080/hook", false).is_err());
        assert!(validate_webhook_url_with("ftp://example.com/hook", true).is_err());
        assert!(validate_webhook_url_with("http://127.0.0.1:8080/hook", true).is_err());
        assert!(validate_webhook_url_with("http://10.0.0.8/hook", true).is_err());
        assert!(validate_webhook_url_with("http://192.168.1.1/hook", true).is_err());
        assert!(validate_webhook_url_with("http://localhost:9000/hook", true).is_err());
        assert!(validate_webhook_url_with("https://hooks.example.com/audit", true).is_ok());
        assert!(validate_webhook_url_with("http://127.0.0.1:9999/hook", false).is_ok());
    }

    /// CGNAT / 文档段 / 保留段 / IPv4 映射地址同样是内网可达地址。
    #[test]
    fn blocks_cgnat_and_mapped_addresses() {
        assert!(validate_webhook_url_with("http://100.64.0.1/hook", true).is_err());
        assert!(validate_webhook_url_with("http://100.127.255.254/hook", true).is_err());
        assert!(validate_webhook_url_with("http://192.0.2.10/hook", true).is_err());
        assert!(validate_webhook_url_with("http://198.18.0.1/hook", true).is_err());
        assert!(validate_webhook_url_with("http://203.0.113.5/hook", true).is_err());
        assert!(validate_webhook_url_with("http://240.0.0.1/hook", true).is_err());
        // IPv4 映射地址必须按 IPv4 规则判：169.254 段无论哪档都拒绝，10/8 归 block_private。
        assert!(validate_webhook_url_with("http://[::ffff:169.254.169.254]/hook", false).is_err());
        assert!(validate_webhook_url_with("http://[::ffff:10.0.0.5]/hook", true).is_err());
    }

    /// 异步档对字面量 IP 直接拒绝，不触发真实 DNS 查询。
    ///
    /// 只断言「与构建档无关」的两类：云元数据段（始终拒绝）与非法协议。
    /// 私网段在 debug 构建是放行的（回环集成测试需要），由同步档测试覆盖。
    #[tokio::test]
    async fn async_validation_rejects_literal_metadata_and_bad_scheme() {
        assert!(
            validate_webhook_url_async("http://169.254.169.254/latest/meta-data")
                .await
                .is_err()
        );
        assert!(validate_webhook_url_async("ftp://example.com/hook")
            .await
            .is_err());
    }

    #[test]
    fn splits_host_and_port() {
        assert_eq!(
            split_host_port("https://Hooks.Example.com./audit").expect("split"),
            ("hooks.example.com".to_string(), 443)
        );
        assert_eq!(
            split_host_port("http://10.0.0.1:8080/hook").expect("split"),
            ("10.0.0.1".to_string(), 8080)
        );
    }
}
