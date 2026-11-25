//! 密码哈希与校验。
//!
//! # 为什么是 Argon2id
//!
//! | 算法 | 问题 |
//! |---|---|
//! | MD5 / SHA1 | 已被撞破，且无盐无迭代，GPU 秒破 |
//! | bcrypt | 72 字节截断、抗 GPU 能力弱于 Argon2 |
//! | PBKDF2 | 可被专用硬件加速 |
//! | **Argon2id** | 抗 GPU/ASIC，内存硬（memory-hard），OWASP 首选 |
//!
//! 参数取 OWASP 推荐值：`m=19456 KiB (19 MiB), t=2, p=1`。
//! 参数写入哈希串本身（PHC 格式），未来调参不会导致旧密码失效。
//!
//! # 安全注意
//!
//! - 哈希串必须存 `text` 列，长度约 100 字节；
//! - 校验失败一律返回 `false`，**不区分**「用户不存在」与「密码错误」；
//! - 生产环境应再加登录失败计数与锁定（见 `docs/01-后端架构.md` 第 9 节）。

use anyhow::Context;
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;

/// 对明文密码做 Argon2id 哈希。
///
/// 每次调用都会生成新的随机盐，因此同一密码两次哈希结果不同（正常现象）。
///
/// **禁止在 async handler 里直接调用**：单次约 19 MiB 内存 + 40~60ms CPU，
/// 会阻塞 tokio worker。请求路径请用 [`hash_password_async`]。
pub fn hash_password(plain: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("密码哈希失败: {e}"))?
        .to_string();
    Ok(hash)
}

/// 校验明文密码与哈希串是否匹配。
///
/// 哈希串格式非法时返回 `false`（视为校验失败），不向外抛错，
/// 避免攻击者通过错误类型判断账号是否存在。
///
/// **禁止在 async handler 里直接调用**，请用 [`verify_password_async`]。
pub fn verify_password(plain: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        tracing::error!("数据库中的密码哈希格式非法，请检查数据完整性");
        return false;
    };
    Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok()
}

/// 异步版密码哈希：在受控阻塞线程池中执行，并限制全局并发。
///
/// 失败语义见 [`crate::blocking::run`]：过载时返回 `Busy`（调用方映射为 429）。
pub async fn hash_password_async(plain: String) -> Result<String, crate::blocking::BlockingError> {
    crate::blocking::run(move || hash_password(&plain))
        .await?
        .map_err(crate::blocking::BlockingError::Failed)
}

/// 异步版密码校验：同样走受控阻塞池。
///
/// 外层 `Result` 表示「是否成功执行校验」，内层 `bool` 才是密码是否正确——
/// 两者不能混淆，否则会把「服务器过载」误报成「密码错误」。
pub async fn verify_password_async(
    plain: String,
    hash: String,
) -> Result<bool, crate::blocking::BlockingError> {
    crate::blocking::run(move || verify_password(&plain, &hash)).await
}

/// 消耗一次与真实校验等价的 Argon2 计算，用于「用户不存在」分支。
///
/// # 为什么需要
///
/// 如果用户不存在时立刻返回 401，而用户存在时要等 40~60ms 的 Argon2，
/// 攻击者只靠响应时间就能枚举出哪些用户名真实存在（时序侧信道）。
/// 这里对一个固定的占位哈希做一次同样的校验，让两条分支耗时对齐。
///
/// 占位哈希在进程内懒生成一次：参数与 [`hash_password`] 完全一致，
/// 因此耗时可对齐；它对应的"密码"是随机不可用的，永远不会校验通过。
pub async fn burn_verification_time() -> Result<(), crate::blocking::BlockingError> {
    static DUMMY_HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    crate::blocking::run(move || {
        // 失败时用空串兜底：`verify_password` 对非法哈希会立即返回 false，
        // 只损失时序保护，不影响功能。
        let hash = DUMMY_HASH.get_or_init(|| {
            hash_password("timing-equalizer-not-a-real-password").unwrap_or_default()
        });
        let _ = verify_password("timing-equalizer-probe", hash);
    })
    .await
}

/// 租户密码策略。
#[derive(Debug, Clone)]
pub struct PasswordPolicy {
    /// 最小长度。
    pub min_length: i32,
    /// 要求大写字母。
    pub require_upper: bool,
    /// 要求小写字母。
    pub require_lower: bool,
    /// 要求数字。
    pub require_digit: bool,
    /// 历史密码保留数量。
    pub history_count: i32,
    /// 密码有效天数（0 表示不过期）。
    pub expires_days: i32,
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        Self {
            min_length: 10,
            require_upper: true,
            require_lower: true,
            require_digit: true,
            history_count: 5,
            expires_days: 0,
        }
    }
}

impl PasswordPolicy {
    /// 按策略校验密码强度。
    pub fn validate(&self, plain: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            plain.chars().count() >= self.min_length.max(1) as usize,
            "密码长度至少 {} 位",
            self.min_length.max(1)
        );
        if self.require_upper {
            anyhow::ensure!(
                plain.chars().any(|c| c.is_ascii_uppercase()),
                "密码必须包含大写字母"
            );
        }
        if self.require_lower {
            anyhow::ensure!(
                plain.chars().any(|c| c.is_ascii_lowercase()),
                "密码必须包含小写字母"
            );
        }
        if self.require_digit {
            anyhow::ensure!(
                plain.chars().any(|c| c.is_ascii_digit()),
                "密码必须包含数字"
            );
        }
        Ok(())
    }
}

/// 读取租户密码策略，未配置时使用默认值。
pub async fn load_policy(
    conn: &mut sqlx::PgConnection,
    tenant_id: uuid::Uuid,
) -> Result<PasswordPolicy, sqlx::Error> {
    let row: Option<(i32, bool, bool, bool, i32, i32)> = sqlx::query_as(
        "SELECT min_length, require_upper, require_lower, require_digit, history_count,
                password_expires_days
         FROM sys.sys_password_policy WHERE tenant_id = $1",
    )
    .bind(tenant_id)
    .fetch_optional(conn)
    .await?;
    Ok(row
        .map(
            |(
                min_length,
                require_upper,
                require_lower,
                require_digit,
                history_count,
                expires_days,
            )| PasswordPolicy {
                min_length,
                require_upper,
                require_lower,
                require_digit,
                history_count,
                expires_days,
            },
        )
        .unwrap_or_default())
}

/// 校验密码强度（默认策略）。
pub fn validate_strength(plain: &str) -> anyhow::Result<()> {
    PasswordPolicy::default().validate(plain)
}

/// 把哈希串补进 anyhow 上下文的便捷封装（供 seed 使用）。
pub fn hash_password_ctx(plain: &str, who: &str) -> anyhow::Result<String> {
    hash_password(plain).with_context(|| format!("为 {who} 生成密码哈希失败"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 哈希后必须能校验通过，且不同次哈希结果不同（随机盐）。
    #[test]
    fn hash_and_verify_roundtrip() {
        let plain = "Admin123456";
        let h1 = hash_password(plain).expect("hash");
        let h2 = hash_password(plain).expect("hash");
        assert_ne!(h1, h2, "随机盐应产生不同哈希");
        assert!(verify_password(plain, &h1));
        assert!(!verify_password("WrongPassword1", &h1));
    }

    /// 非法哈希串不能让进程 panic。
    #[test]
    fn invalid_hash_returns_false() {
        assert!(!verify_password("x", "not-a-phc-string"));
    }

    /// 强度校验：长度与字符类别。
    #[test]
    fn strength_rules() {
        assert!(validate_strength("Admin123456").is_ok());
        assert!(validate_strength("short1A").is_err());
        assert!(validate_strength("alllowercase1").is_err());
    }
}
