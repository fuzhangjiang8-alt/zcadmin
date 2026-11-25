//! 审计告警订阅投递：按用户订阅匹配并写入投递队列。
//!
//! 边界：Webhook 实际投递在 `audit_notify`。
//! 关键约束：逐租户设置 RLS 上下文；模式匹配失败按非法数据处理。

use sqlx::PgPool;

use crate::audit_notify::{sign_payload, DeliveryRow};
use uuid::Uuid;

/// 将投递错误归一化为稳定分类。
pub fn classify_error(raw: &str) -> &'static str {
    if let Some(code) = raw.strip_prefix("HTTP ") {
        return match code.trim().parse::<u16>() {
            Ok(400..=499) => "http_4xx",
            Ok(500..=599) => "http_5xx",
            _ => "http_other",
        };
    }
    let lower = raw.to_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        "timeout"
    } else if lower.contains("connect")
        || lower.contains("refused")
        || lower.contains("dns")
        || lower.contains("unreachable")
    {
        "network"
    } else {
        "other"
    }
}

/// 按订阅分发告警到用户级 Webhook。
///
/// 与租户级 Webhook 独立：为每条匹配订阅创建 delivery 记录，
/// 失败按 1/2/4/8/16/30 分钟退避，8 次后进入 dead。
pub async fn dispatch_subscriptions(pool: &PgPool) -> anyhow::Result<u64> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let tenants: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM sys.sys_tenant")
        .fetch_all(pool)
        .await?;
    let mut sent = 0u64;

    for tenant in tenants {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;

        sqlx::query(&format!(
            r#"
            INSERT INTO sys.sys_audit_alert_delivery
                (id, tenant_id, alert_id, subscription_id, url, secret)
            SELECT gen_random_uuid(), a.tenant_id, a.id, s.id, c.url, c.secret
            FROM sys.sys_audit_alert a
            JOIN sys.sys_audit_alert_subscription s
              ON s.tenant_id = a.tenant_id
             AND a.action LIKE {} ESCAPE '\'
            JOIN sys.sys_audit_alert_subscription_config c
              ON c.tenant_id = s.tenant_id AND c.user_id = s.user_id
            WHERE a.tenant_id = $1
              AND a.created_at >= now() - interval '30 days'
              AND a.action NOT LIKE 'system.delivery.%'
              AND c.enabled
              AND btrim(c.url) <> ''
              AND NOT EXISTS (
                  SELECT 1 FROM sys.sys_audit_alert_delivery d
                  WHERE d.alert_id = a.id AND d.subscription_id = s.id
              )
            ON CONFLICT DO NOTHING
            "#,
            // 订阅模式是用户输入：必须转义 `%`/`_`，否则订阅 `%` 等于订阅全租户告警。
            crate::audit_subscriptions::like_expression("s")
        ))
        .bind(tenant)
        .execute(&mut *tx)
        .await?;

        // 多实例并发安全：FOR UPDATE SKIP LOCKED 抢占待投递记录并设置 60s 租约
        let rows: Vec<DeliveryRow> = sqlx::query_as(
            r#"
            WITH claimed AS (
                SELECT d.id
                FROM sys.sys_audit_alert_delivery d
                WHERE d.tenant_id = $1
                  AND d.status = 'pending'
                  AND (d.next_attempt_at IS NULL OR d.next_attempt_at <= now())
                ORDER BY d.created_at
                LIMIT 20
                FOR UPDATE SKIP LOCKED
            )
            UPDATE sys.sys_audit_alert_delivery d
            SET next_attempt_at = now() + interval '60 seconds'
            FROM claimed c, sys.sys_audit_alert a
            WHERE d.id = c.id AND d.tenant_id = $1 AND a.id = d.alert_id
            RETURNING d.id, d.url, d.secret, d.attempts,
                      a.action, a.count, a.window_start::text, a.window_end::text
            "#,
        )
        .bind(tenant)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;

        for (id, url, secret, attempts, action, count, window_start, window_end) in rows {
            // 发送前复检（含域名解析）：配置写入时的校验挡不住「配置包导入覆盖」
            // 与「域名后来改指内网」。策略拒绝不重试，直接进死信。
            if let Err(reason) = crate::ssrf::validate_webhook_url_async(&url).await {
                tracing::warn!(%id, %url, %reason, "投递地址未通过 SSRF 校验，标记死信");
                crate::audit_notify::mark_dead(
                    pool,
                    tenant,
                    id,
                    attempts.saturating_add(1),
                    &reason,
                    "ssrf_blocked",
                )
                .await?;
                continue;
            }
            let body = serde_json::json!({
                "id": id,
                "action": action,
                "count": count,
                "windowStart": window_start,
                "windowEnd": window_end
            })
            .to_string();
            let mut request = client
                .post(&url)
                .header("content-type", "application/json")
                .body(body.clone());
            if let Some(secret) = secret.as_deref().filter(|s| !s.is_empty()) {
                let signature = sign_payload(secret, &body)?;
                request = request.header("x-signature", format!("sha256={signature}"));
            }

            let outcome: Result<(), String> = match request.send().await {
                Ok(resp) if resp.status().is_success() => Ok(()),
                Ok(resp) => Err(format!("HTTP {}", resp.status())),
                Err(e) => Err(e.to_string()),
            };
            let mut mark = pool.begin().await?;
            sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
                .bind(tenant.to_string())
                .execute(&mut *mark)
                .await?;
            if outcome.is_ok() {
                sqlx::query(
                    "UPDATE sys.sys_audit_alert_delivery
                     SET status = 'sent', sent_at = now(), attempts = $3, last_error = NULL
                     WHERE tenant_id = $1 AND id = $2",
                )
                .bind(tenant)
                .bind(id)
                .bind(attempts + 1)
                .execute(&mut *mark)
                .await?;
                sent += 1;
            } else {
                let next = attempts.saturating_add(1);
                let delay = (60i64 * 2i64.pow(next.min(5) as u32)).min(1800);
                let error = outcome.err().unwrap_or_default();
                let error_kind = classify_error(&error);
                if next >= 8 {
                    sqlx::query(
                        "UPDATE sys.sys_audit_alert_delivery
                         SET status = 'dead', attempts = $3, last_error = $4, last_error_kind = $5
                         WHERE tenant_id = $1 AND id = $2",
                    )
                    .bind(tenant)
                    .bind(id)
                    .bind(next)
                    .bind(&error)
                    .bind(error_kind)
                    .execute(&mut *mark)
                    .await?;
                    // 死信本身再生成一条审计告警（订阅分发已排除该动作，避免递归）
                    let delivery_silence: i32 = sqlx::query_scalar(
                        r#"
                        SELECT COALESCE((
                            SELECT silence_minutes FROM sys.sys_audit_alert_rule
                            WHERE tenant_id = $1 AND enabled
                              AND 'system.delivery.dead' LIKE replace(action_pattern, '*', '%')
                            ORDER BY length(action_pattern) DESC
                            LIMIT 1
                        ), 30)
                        "#,
                    )
                    .bind(tenant)
                    .fetch_one(&mut *mark)
                    .await?;
                    sqlx::query(
                        r#"
                        INSERT INTO sys.sys_audit_alert
                            (id, tenant_id, action, window_start, window_end, count, threshold)
                        SELECT gen_random_uuid(), $1, 'system.delivery.dead',
                               date_trunc('hour', now())
                                 + (floor(extract(minute from now()) / 5) * 5) * interval '1 minute',
                               now(), 1, 1
                        WHERE NOT EXISTS (
                            SELECT 1 FROM sys.sys_audit_alert a
                            WHERE a.tenant_id = $1
                              AND a.action = 'system.delivery.dead'
                              AND a.created_at >= now() - make_interval(mins => $2)
                        )
                        ON CONFLICT (tenant_id, action, window_start)
                        DO UPDATE SET count = sys.sys_audit_alert.count + 1,
                                      window_end = now()
                        "#,
                    )
                    .bind(tenant)
                    .bind(delivery_silence)
                    .execute(&mut *mark)
                    .await?;
                } else {
                    sqlx::query(
                        "UPDATE sys.sys_audit_alert_delivery
                         SET attempts = $3, next_attempt_at = now() + make_interval(secs => $4),
                             last_error = $5, last_error_kind = $6
                         WHERE tenant_id = $1 AND id = $2",
                    )
                    .bind(tenant)
                    .bind(id)
                    .bind(next)
                    .bind(delay as i32)
                    .bind(&error)
                    .bind(error_kind)
                    .execute(&mut *mark)
                    .await?;
                }
            }
            mark.commit().await?;
        }
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::classify_error;
    use crate::audit_notify::{render_template, sign_payload};
    use uuid::Uuid;

    #[test]
    fn classify_error_buckets() {
        assert_eq!(classify_error("HTTP 500"), "http_5xx");
        assert_eq!(classify_error("HTTP 404"), "http_4xx");
        assert_eq!(classify_error("operation timed out"), "timeout");
        assert_eq!(classify_error("connection refused"), "network");
        assert_eq!(classify_error("something weird"), "other");
    }

    #[test]
    fn render_template_replaces_all_placeholders() {
        let id = Uuid::nil();
        let out = render_template(
            r#"{"a":"{{action}}","n":{{count}},"s":"{{windowStart}}"}"#,
            id,
            "auth.login.failed",
            7,
            "2026-01-01",
            "2026-01-02",
        );
        assert_eq!(out, r#"{"a":"auth.login.failed","n":7,"s":"2026-01-01"}"#);
    }

    #[test]
    fn sign_payload_matches_known_vector() {
        let sig = sign_payload("key", "The quick brown fox jumps over the lazy dog").unwrap();
        assert_eq!(
            sig,
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }
}
