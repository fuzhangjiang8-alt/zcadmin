//! 登录策略：密码有效期等租户级配置。
//!
//! 边界：登录流程在 `auth`；本模块只加载策略数据。
//! 关键约束：策略按租户读取，缺失时使用默认值。

use uuid::Uuid;

/// 租户登录风控策略。
#[derive(Debug, Clone)]
pub struct LoginPolicy {
    /// 最大失败次数。
    pub max_failed_attempts: i32,
    /// 锁定秒数。
    pub lock_seconds: i64,
    /// 预警百分比。
    pub warning_percent: i32,
}

impl Default for LoginPolicy {
    fn default() -> Self {
        Self {
            max_failed_attempts: 5,
            lock_seconds: 900,
            warning_percent: 60,
        }
    }
}

/// 读取租户登录策略，未配置时使用默认值。
pub async fn load_login_policy(
    conn: &mut sqlx::PgConnection,
    tenant_id: Uuid,
) -> Result<LoginPolicy, sqlx::Error> {
    let row: Option<(i32, i32, i32)> = sqlx::query_as(
        "SELECT max_failed_attempts, lock_seconds, warning_percent
         FROM sys.sys_login_policy WHERE tenant_id = $1",
    )
    .bind(tenant_id)
    .fetch_optional(conn)
    .await?;
    Ok(row
        .map(
            |(max_failed_attempts, lock_seconds, warning_percent)| LoginPolicy {
                max_failed_attempts,
                lock_seconds: i64::from(lock_seconds),
                warning_percent,
            },
        )
        .unwrap_or_default())
}

/// 若本次登录 IP 或 User-Agent 与上次成功登录不一致，写入新设备登录告警。
pub(crate) async fn record_new_device_if_changed(
    conn: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    user_id: Uuid,
    client_ip: Option<&str>,
    user_agent: Option<&str>,
) -> Result<(), crate::error::AppError> {
    let previous_login: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT host(ip), user_agent FROM sys.sys_audit_log
         WHERE tenant_id = $1 AND user_id = $2 AND action = 'auth.login.success'
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_optional(&mut *conn)
    .await?;
    let new_device = previous_login
        .map(|(ip, ua)| ip.as_deref() != client_ip || ua.as_deref() != user_agent)
        .unwrap_or(false);
    if new_device {
        sqlx::query(
            "INSERT INTO sys.sys_audit_alert (id, tenant_id, user_id, action, window_start, window_end, count, threshold)
             VALUES (gen_random_uuid(), $1, $2, 'auth.login.new_device', date_trunc('minute', now()), now(), 1, 1)
             ON CONFLICT (tenant_id, action, window_start)
             DO UPDATE SET count = sys.sys_audit_alert.count + 1, window_end = now()",
        )
        .bind(tenant_id)
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}
