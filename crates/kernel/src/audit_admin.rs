//! 审计告警、订阅、投递与审计配置快照。
//!
//! # 职责
//!
//! - 审计统计与告警（规则、订阅、Webhook、投递重试）；
//! - 审计配置的导出/导入/预览、快照对比与清理。
//!
//! # 边界
//!
//! 审计日志本身的查询（`list_audit_logs` / `list_audit_actions`）仍在 `system` 模块；
//! 本模块只处理「告警与配置」这一层，所有写接口要求 `system:audit:alert` 权限。
//!
//! # 关键约束
//!
//! 从 `system.rs` 拆出（原文件 5.5k 行）。路由由 [`router`] 提供，
//! OpenAPI 片段由 [`AuditAdminApi`] 提供；`SystemApi` 不再包含这些路径。

use crate::audit_config_bundle::{AuditConfigBundle, AuditConfigDiffVo};
use crate::audit_stats::{ActionCountVo, AlertUserStat, AuditStatsQuery, DayCountVo};
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::Router;
use serde::Serialize;
use uuid::Uuid;

/// 审计告警/配置路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .merge(crate::audit_stats::router())
        .merge(crate::audit_rules::router())
        .merge(crate::audit_alert_settings::router())
        .merge(crate::audit_config_export::router())
        .merge(crate::audit_config_import::router())
        .merge(crate::audit_config_preview::router())
        .merge(crate::audit_config_snapshots::router())
        .merge(crate::audit_config_snapshot_tools::router())
        .merge(crate::audit_alerts::router())
        .merge(crate::audit_subscriptions::router())
        .merge(crate::audit_deliveries::router())
        .route("/audit-alerts/stats", get(audit_alert_stats))
        .route("/audit-alerts/{id}/retry", post(retry_audit_alert))
}

/// 告警统计。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertStatsVo {
    /// 统计天数。
    pub days: i64,
    /// 总数。
    pub total: i64,
    /// 未处理数。
    pub unacked: i64,
    /// 已处理数。
    pub acknowledged: i64,
    /// 平均处理时长（分钟，已处理样本）。
    pub avg_ack_minutes: f64,
    /// Webhook 投递失败（死信）数量。
    pub notify_failed: i64,
    /// 按动作计数。
    pub by_action: Vec<ActionCountVo>,
    /// 按天计数。
    pub by_day: Vec<DayCountVo>,
    /// 按用户计数（Top 5）。
    pub by_user: Vec<AlertUserStat>,
}

/// 告警统计。
#[utoipa::path(
    get,
    path = "/api/system/audit-alerts/stats",
    tag = "system",
    params(("days" = Option<i64>, Query, description = "最近天数，默认 30")),
    responses((status = 200, description = "告警统计", body = ApiResponse<AlertStatsVo>))
)]
async fn audit_alert_stats(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<AuditStatsQuery>,
) -> Result<ApiResponse<AlertStatsVo>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let days = q.days.unwrap_or(30).clamp(1, 365);

    let (total, unacked, acknowledged, avg_ack_minutes, notify_failed): (
        i64,
        i64,
        i64,
        Option<f64>,
        i64,
    ) = sqlx::query_as(
        r#"
            SELECT count(*)::bigint,
                   count(*) FILTER (WHERE acknowledged_at IS NULL)::bigint,
                   count(*) FILTER (WHERE acknowledged_at IS NOT NULL)::bigint,
                   avg(EXTRACT(EPOCH FROM (acknowledged_at - created_at)) / 60.0)
                       FILTER (WHERE acknowledged_at IS NOT NULL)::float8,
                   count(*) FILTER (WHERE notify_failed_at IS NOT NULL)::bigint
            FROM sys.sys_audit_alert
            WHERE tenant_id = $1 AND created_at >= now() - make_interval(days => $2::int)
            "#,
    )
    .bind(user.tenant_id)
    .bind(days as i32)
    .fetch_one(&mut *tx)
    .await?;

    let by_user: Vec<AlertUserStat> = sqlx::query_as(
        r#"
        SELECT u.username, count(*)::bigint AS count
        FROM sys.sys_audit_alert a
        JOIN sys.sys_user u ON u.id = a.user_id
        WHERE a.tenant_id = $1 AND a.created_at >= now() - make_interval(days => $2::int)
        GROUP BY u.username
        ORDER BY count DESC, u.username
        LIMIT 5
        "#,
    )
    .bind(user.tenant_id)
    .bind(days as i32)
    .fetch_all(&mut *tx)
    .await?;

    let by_day: Vec<DayCountVo> = sqlx::query_as(
        r#"
        SELECT to_char(created_at::date, 'YYYY-MM-DD') AS day, count(*)::bigint AS count
        FROM sys.sys_audit_alert
        WHERE tenant_id = $1 AND created_at >= now() - make_interval(days => $2::int)
        GROUP BY created_at::date
        ORDER BY created_at::date
        "#,
    )
    .bind(user.tenant_id)
    .bind(days as i32)
    .fetch_all(&mut *tx)
    .await?;

    let by_action: Vec<ActionCountVo> = sqlx::query_as(
        r#"
        SELECT action, count(*)::bigint AS count
        FROM sys.sys_audit_alert
        WHERE tenant_id = $1 AND created_at >= now() - make_interval(days => $2::int)
        GROUP BY action
        ORDER BY count DESC, action
        LIMIT 10
        "#,
    )
    .bind(user.tenant_id)
    .bind(days as i32)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(AlertStatsVo {
        days,
        total,
        unacked,
        acknowledged,
        avg_ack_minutes: avg_ack_minutes.unwrap_or(0.0),
        notify_failed,
        by_action,
        by_day,
        by_user,
    }))
}

/// 重投死信告警。
#[utoipa::path(
    post,
    path = "/api/system/audit-alerts/{id}/retry",
    tag = "system",
    params(("id" = Uuid, Path, description = "告警 id")),
    responses((status = 200, description = "已重置投递状态"))
)]
async fn retry_audit_alert(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::AUDIT_ALERT)?;
    let result = sqlx::query(
        "UPDATE sys.sys_audit_alert
         SET notify_failed_at = NULL, notify_attempts = 0, next_notify_at = NULL
         WHERE id = $1 AND tenant_id = $2 AND notify_failed_at IS NOT NULL",
    )
    .bind(id)
    .bind(user.tenant_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.audit.alert.retry",
        "audit_alert",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 审计告警/配置文件接口的 OpenAPI 片段。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        crate::audit_stats::audit_stats,
        audit_alert_stats,
        crate::audit_deliveries::audit_delivery_stats,
        crate::audit_alerts::list_audit_alerts,
        crate::audit_deliveries::list_audit_deliveries,
        crate::audit_deliveries::retry_audit_delivery,
        crate::audit_deliveries::retry_all_dead_deliveries,
        crate::audit_alerts::ack_audit_alert,
        retry_audit_alert,
        crate::audit_subscriptions::list_alert_subscriptions,
        crate::audit_subscriptions::replace_alert_subscriptions,
        crate::audit_subscriptions::get_subscription_config,
        crate::audit_subscriptions::update_subscription_config,
        crate::audit_alerts::list_subscribed_alerts,
        crate::audit_alert_settings::get_alert_config,
        crate::audit_alert_settings::update_alert_config,
        crate::audit_alert_settings::get_audit_webhook,
        crate::audit_alert_settings::update_audit_webhook,
        crate::audit_rules::list_audit_rules,
        crate::audit_rules::replace_audit_rules,
        crate::audit_rules::preview_audit_rule,
        crate::audit_config_export::export_audit_config,
        crate::audit_config_import::import_audit_config,
        crate::audit_config_preview::preview_audit_config,
        crate::audit_config_snapshots::list_config_snapshots,
        crate::audit_config_snapshots::delete_config_snapshot,
        crate::audit_config_snapshot_tools::update_config_snapshot,
        crate::audit_config_snapshots::cleanup_config_snapshots,
        crate::audit_config_snapshot_tools::export_config_snapshot,
        crate::audit_config_snapshot_tools::compare_config_snapshots
    ),
    components(schemas(
        crate::audit_stats::AuditStatsVo,
        crate::audit_stats::ActionCountVo,
        crate::audit_stats::DayCountVo,
        crate::audit_stats::AuditStatsQuery,
        AlertStatsVo,
        crate::audit_stats::AlertUserStat,
        crate::audit_deliveries::DeliveryStatsVo,
        crate::audit_deliveries::DeliveryUserStat,
        crate::audit_deliveries::DeliveryErrorStat,
        crate::audit_deliveries::DeliveryVo,
        crate::audit_deliveries::DeliveryQuery,
        crate::audit_alerts::AlertVo,
        crate::audit_alerts::AuditAlertQuery,
        crate::audit_alert_settings::AlertConfigVo,
        crate::audit_alert_settings::WebhookVo,
        crate::audit_alert_settings::WebhookRequest,
        crate::audit_rules::AuditAlertRuleVo,
        crate::audit_rules::AuditAlertRuleInput,
        crate::audit_rules::ReplaceRulesRequest,
        AuditConfigBundle,
        AuditConfigDiffVo,
        crate::audit_config_snapshots::SnapshotVo,
        crate::audit_config_snapshots::SnapshotQuery,
        crate::audit_config_snapshots::SnapshotCleanupQuery,
        crate::audit_config_snapshot_tools::SnapshotCompareQuery,
        crate::audit_config_snapshot_tools::SnapshotUpdateRequest,
        crate::audit_rules::RulePreviewRequest,
        crate::audit_rules::RulePreviewVo,
        crate::audit_subscriptions::SubscriptionVo,
        crate::audit_subscriptions::SubscriptionRequest,
        crate::audit_subscriptions::SubscriptionConfigVo,
        crate::audit_subscriptions::SubscriptionConfigRequest
    ))
)]
pub struct AuditAdminApi;
