//! 审计告警通知：Webhook 投递、签名、模板渲染。
//!
//! 边界：告警检测在 `audit`；订阅匹配与投递在 `audit_subscription_dispatch`。
//! 关键约束：失败重试带退避；secret 只用于签名，不落日志。

use sqlx::PgPool;
use uuid::Uuid;

/// 标记 Webhook 投递失败并按指数退避安排下次重试。
async fn mark_failed(
    pool: &PgPool,
    tenant: Uuid,
    alert_id: Uuid,
    attempts: i32,
) -> anyhow::Result<()> {
    const MAX_NOTIFY_ATTEMPTS: i32 = 8;
    let next = attempts.saturating_add(1);
    let delay = (60i64 * 2i64.pow(next.min(5) as u32)).min(1800);
    let dead = next >= MAX_NOTIFY_ATTEMPTS;
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await?;
    if dead {
        sqlx::query(
            "UPDATE sys.sys_audit_alert
             SET notify_attempts = $3, next_notify_at = NULL, notify_failed_at = now()
             WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant)
        .bind(alert_id)
        .bind(next)
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query(
            "UPDATE sys.sys_audit_alert
             SET notify_attempts = $3,
                 next_notify_at = now() + make_interval(secs => $4)
             WHERE tenant_id = $1 AND id = $2",
        )
        .bind(tenant)
        .bind(alert_id)
        .bind(next)
        .bind(delay as i32)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// 立即把一条投递标记为终态失败。
///
/// 用于「重试也不会成功」的原因（例如目标地址被 SSRF 策略拒绝）：
/// 继续按退避重试只会在每轮里多打一次内网。
pub(crate) async fn mark_dead(
    pool: &PgPool,
    tenant: Uuid,
    id: Uuid,
    attempts: i32,
    error: &str,
    kind: &str,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE sys.sys_audit_alert_delivery
         SET status = 'dead', attempts = $3, last_error = $4, last_error_kind = $5
         WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant)
    .bind(id)
    .bind(attempts)
    .bind(error)
    .bind(kind)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// 计算 Webhook HMAC-SHA256 签名（十六进制）。
pub(crate) fn sign_payload(secret: &str, body: &str) -> anyhow::Result<String> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())?;
    mac.update(body.as_bytes());
    Ok(mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// 渲染 Webhook 模板。
///
/// 占位符：`{{id}}` / `{{action}}` / `{{count}}` / `{{windowStart}}` / `{{windowEnd}}`。
pub fn render_template(
    template: &str,
    id: Uuid,
    action: &str,
    count: i64,
    window_start: &str,
    window_end: &str,
) -> String {
    template
        .replace("{{id}}", &id.to_string())
        .replace("{{action}}", action)
        .replace("{{count}}", &count.to_string())
        .replace("{{windowStart}}", window_start)
        .replace("{{windowEnd}}", window_end)
}

/// 推送未通知的告警到租户 Webhook。
///
/// 签名：`X-Signature: sha256=<hex(HMAC-SHA256(secret, body))>`。
/// 只推送最近 20 条未通知告警；HTTP 非 2xx 视为失败，下轮重试。
pub async fn notify_pending(pool: &PgPool) -> anyhow::Result<u64> {
    // 禁用 HTTP 重定向：防止通过公网跳板 302 重定向到 169.254.169.254 等内网地址绕过 SSRF 校验
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM sys.sys_tenant")
        .fetch_all(pool)
        .await?;
    let mut notified = 0u64;

    for tenant in tenants {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        let hook: Option<(String, Option<String>, bool, Option<String>)> = sqlx::query_as(
            "SELECT url, secret, enabled, payload_template FROM sys.sys_audit_webhook WHERE tenant_id = $1",
        )
        .bind(tenant)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((url, secret, true, payload_template)) = hook else {
            tx.commit().await?;
            continue;
        };
        // 发送前复检（含域名解析）：配置写入时的校验不足以覆盖「域名后来指向内网」，
        // 也不能覆盖「配置被导入/直接改库绕过校验」。
        if url.trim().is_empty() || crate::ssrf::validate_webhook_url_async(&url).await.is_err() {
            tracing::warn!(%tenant, "Webhook 地址未通过 SSRF 校验，跳过本轮投递");
            tx.commit().await?;
            continue;
        }

        // 多实例并发安全：FOR UPDATE SKIP LOCKED 抢占最多 20 条记录并续期 60s 租约，
        // 避免多个副本在同一扫描周期拉出相同告警重复推送。
        let rows: Vec<(Uuid, String, i64, String, String, i32)> = sqlx::query_as(
            r#"
            WITH claimed AS (
                SELECT id
                FROM sys.sys_audit_alert
                WHERE tenant_id = $1
                  AND notified_at IS NULL
                  AND notify_failed_at IS NULL
                  AND (next_notify_at IS NULL OR next_notify_at <= now())
                ORDER BY created_at
                LIMIT 20
                FOR UPDATE SKIP LOCKED
            )
            UPDATE sys.sys_audit_alert a
            SET next_notify_at = now() + interval '60 seconds'
            FROM claimed c
            WHERE a.id = c.id AND a.tenant_id = $1
            RETURNING a.id, a.action, a.count, a.window_start::text, a.window_end::text, a.notify_attempts
            "#,
        )
        .bind(tenant)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;

        for (id, action, count, window_start, window_end, attempts) in rows {
            let body_text = match payload_template.as_deref().filter(|t| !t.trim().is_empty()) {
                Some(template) => {
                    render_template(template, id, &action, count, &window_start, &window_end)
                }
                None => serde_json::json!({
                    "id": id,
                    "action": action,
                    "count": count,
                    "windowStart": window_start,
                    "windowEnd": window_end
                })
                .to_string(),
            };
            let mut request = client
                .post(&url)
                .header("content-type", "application/json")
                .body(body_text.clone());
            if let Some(secret) = secret.as_deref().filter(|s| !s.is_empty()) {
                let signature = sign_payload(secret, &body_text)?;
                request = request.header("x-signature", format!("sha256={signature}"));
            }

            match request.send().await {
                Ok(resp) if resp.status().is_success() => {
                    let mut mark = pool.begin().await?;
                    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
                        .bind(tenant.to_string())
                        .execute(&mut *mark)
                        .await?;
                    sqlx::query(
                        "UPDATE sys.sys_audit_alert
                         SET notified_at = now(), notify_attempts = $3
                         WHERE tenant_id = $1 AND id = $2",
                    )
                    .bind(tenant)
                    .bind(id)
                    .bind(attempts + 1)
                    .execute(&mut *mark)
                    .await?;
                    mark.commit().await?;
                    notified += 1;
                }
                Ok(resp) => {
                    tracing::warn!(status = %resp.status(), %url, "Webhook 返回非 2xx");
                    mark_failed(pool, tenant, id, attempts).await?;
                }
                Err(e) => {
                    tracing::warn!(error = %e, %url, "Webhook 推送失败");
                    mark_failed(pool, tenant, id, attempts).await?;
                }
            }
        }
    }
    Ok(notified)
}

pub(crate) type DeliveryRow = (
    Uuid,
    String,
    Option<String>,
    i32,
    String,
    i64,
    String,
    String,
);
