//! 敏感字段脱敏。
//!
//! # 为什么在服务端做
//!
//! 脱敏必须发生在**序列化之前**。如果前端拿到明文再去遮，打开浏览器调试器就能看到原文，
//! 等于没做。这里的函数只处理"输出"，不处理存储。
//!
//! # 规则
//!
//! | 字段 | 规则 | 示例 |
//! |---|---|---|
//! | 邮箱 | 保留前 2 位 + `***` + 域名 | `zhangsan@a.com` → `zh***@a.com` |
//! | 手机号 | 保留前 3 后 4 | `13812345678` → `138****5678` |
//!
//! 对于长度不足的输入，退化为全掩码（不泄漏长度信息）。

/// 邮箱脱敏。
pub fn email(value: &str) -> String {
    match value.split_once('@') {
        Some((local, domain)) => {
            let visible: String = local.chars().take(2).collect();
            format!("{visible}***@{domain}")
        }
        // 不含 @ 的异常数据：整体掩码，避免把脏数据原样吐出去
        None => "***".to_string(),
    }
}

/// 手机号脱敏。
pub fn phone(value: &str) -> String {
    let digits: Vec<char> = value.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 7 {
        return "***".to_string();
    }
    let head: String = digits.iter().take(3).collect();
    let tail: String = digits.iter().skip(digits.len() - 4).collect();
    format!("{head}****{tail}")
}

/// 规范化中国大陆手机号：去掉空格/短横线，允许 +86 前缀。
///
/// 返回 None 表示格式不合法；合法时返回 11 位纯数字。
pub fn normalize_phone(value: &str) -> Option<String> {
    let cleaned: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '-' | '(' | ')'))
        .collect();
    let digits = cleaned
        .strip_prefix("+86")
        .or_else(|| cleaned.strip_prefix("86"))
        .unwrap_or(&cleaned);
    if digits.len() != 11 || !digits.starts_with('1') {
        return None;
    }
    let second = digits.as_bytes()[1];
    if !(b'3'..=b'9').contains(&second) || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(digits.to_string())
}

/// 通用文本脱敏：保留首尾各 `keep` 个字符，中间用 `***` 替代。
pub fn text(value: &str, keep: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= keep * 2 {
        return "***".to_string();
    }
    let head: String = chars.iter().take(keep).collect();
    let tail: String = chars.iter().skip(chars.len() - keep).collect();
    format!("{head}***{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_masks_local_part() {
        assert_eq!(email("zhangsan@example.com"), "zh***@example.com");
        assert_eq!(email("a@b.com"), "a***@b.com");
        assert_eq!(email("not-an-email"), "***");
    }

    #[test]
    fn phone_keeps_head_and_tail() {
        assert_eq!(phone("13812345678"), "138****5678");
        assert_eq!(phone("138-1234-5678"), "138****5678");
        assert_eq!(phone("123"), "***");
    }

    #[test]
    fn phone_normalization() {
        assert_eq!(
            normalize_phone("13812345678").as_deref(),
            Some("13812345678")
        );
        assert_eq!(
            normalize_phone("+86 138-1234-5678").as_deref(),
            Some("13812345678")
        );
        assert_eq!(normalize_phone("12812345678"), None);
        assert_eq!(normalize_phone("123"), None);
    }

    #[test]
    fn text_handles_short_input() {
        assert_eq!(text("abcdef", 2), "ab***ef");
        assert_eq!(text("abc", 2), "***");
    }
}
