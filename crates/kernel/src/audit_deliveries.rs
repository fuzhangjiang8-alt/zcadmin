//! 审计投递记录：统计、列表、单条重试与死信批量重试。
//!
//! 边界：告警规则/订阅在各自模块；本模块只处理投递记录。
//! 关键约束：重试只针对 pending/dead，成功态不可重复投递。

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::audit_stats::AuditStatsQuery;
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::{ApiResponse, Page, PageMeta, PageQuery};
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;

/// 审计投递路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/audit-delivery-stats", get(audit_delivery_stats))
        .route("/audit-deliveries", get(list_audit_deliveries))
        .route(
            "/audit-deliveries/retry-dead",
            post(retry_all_dead_deliveries),
        )
        .route("/audit-deliveries/{id}/retry", post(retry_audit_delivery))
}

/// 订阅投递统计。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryStatsVo {
    /// 统计天数。
    pub days: i64,
    /// 总投递数。
    pub total: i64,
    /// 待投递。
    pub pending: i64,
    /// 已投递。
    pub sent: i64,
    /// 死信。
    pub dead: i64,
    /// 平均尝试次数。
    pub avg_attempts: f64,
    /// 按订阅用户统计（Top 10）。
    pub by_user: Vec<DeliveryUserStat>,
    /// 死信原因（Top 10）。
    pub by_error: Vec<DeliveryErrorStat>,
}

/// 死信原因统计。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryErrorStat {
    /// 错误信息。
    pub error: String,
    /// 数量。
    pub count: i64,
}

/// 用户投递统计。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryUserStat {
    /// 登录名。
    pub username: String,
    /// 总投递数。
    pub total: i64,
    /// 成功数。
    pub sent: i64,
    /// 死信数。
    pub dead: i64,
}

/// 订阅投递监控。
#[utoipa::path(
    get,
    path = "/api/system/audit-delivery-stats",
    tag = "system",
    params(("days" = Option<i64>, Query, description = "最近天数，默认 30")),
    responses((status = 200, description = "投递统计", body = ApiResponse<DeliveryStatsVo>))
)]
async fn audit_delivery_stats(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<AuditStatsQuery>,
) -> Result<ApiResponse<DeliveryStatsVo>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let days = q.days.unwrap_or(30).clamp(1, 365);
    let (total, pending, sent, dead, avg_attempts): (i64, i64, i64, i64, Option<f64>) =
        sqlx::query_as(
            "SELECT count(*)::bigint,
                    count(*) FILTER (WHERE status = 'pending')::bigint,
                    count(*) FILTER (WHERE status = 'sent')::bigint,
                    count(*) FILTER (WHERE status = 'dead')::bigint,
                    avg(attempts)::float8
             FROM sys.sys_audit_alert_delivery
             WHERE tenant_id = $1 AND created_at >= now() - make_interval(days => $2::int)",
        )
        .bind(user.tenant_id)
        .bind(days as i32)
        .fetch_one(&mut *tx)
        .await?;
    let by_user: Vec<DeliveryUserStat> = sqlx::query_as(
        "SELECT u.username,
                count(*)::bigint AS total,
                count(*) FILTER (WHERE d.status = 'sent')::bigint AS sent,
                count(*) FILTER (WHERE d.status = 'dead')::bigint AS dead
         FROM sys.sys_audit_alert_delivery d
         JOIN sys.sys_audit_alert_subscription s ON s.id = d.subscription_id
         JOIN sys.sys_user u ON u.id = s.user_id
         WHERE d.tenant_id = $1 AND d.created_at >= now() - make_interval(days => $2::int)
         GROUP BY u.username ORDER BY total DESC, u.username LIMIT 10",
    )
    .bind(user.tenant_id)
    .bind(days as i32)
    .fetch_all(&mut *tx)
    .await?;

    let by_error: Vec<DeliveryErrorStat> = sqlx::query_as(
        "SELECT COALESCE(last_error_kind, 'unknown') AS error, count(*)::bigint AS count
         FROM sys.sys_audit_alert_delivery
         WHERE tenant_id = $1 AND status = 'dead'
           AND created_at >= now() - make_interval(days => $2::int)
         GROUP BY last_error_kind ORDER BY count DESC LIMIT 10",
    )
    .bind(user.tenant_id)
    .bind(days as i32)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(DeliveryStatsVo {
        days,
        total,
        pending,
        sent,
        dead,
        avg_attempts: avg_attempts.unwrap_or(0.0),
        by_user,
        by_error,
    }))
}

/// 订阅投递记录。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryVo {
    /// 投递 id。
    pub id: Uuid,
    /// 告警 id。
    pub alert_id: Uuid,
    /// 动作。
    pub action: String,
    /// 回调地址。
    pub url: String,
    /// 状态。
    pub status: String,
    /// 尝试次数。
    pub attempts: i32,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 投递查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryQuery {
    /// 分页参数。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 状态筛选（pending/sent/dead）。
    pub status: Option<String>,
}

/// 订阅投递列表。
#[utoipa::path(
    get,
    path = "/api/system/audit-deliveries",
    tag = "system",
    responses((status = 200, description = "投递分页", body = ApiResponse<Page<DeliveryVo>>))
)]
async fn list_audit_deliveries(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<DeliveryQuery>,
) -> Result<ApiResponse<Page<DeliveryVo>>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let rows: Vec<DeliveryVo> = sqlx::query_as(
        "SELECT d.id, d.alert_id, a.action, d.url, d.status, d.attempts, d.created_at
         FROM sys.sys_audit_alert_delivery d
         JOIN sys.sys_audit_alert a ON a.id = d.alert_id
         WHERE d.tenant_id = $1 AND ($2::text IS NULL OR d.status = $2)
         ORDER BY d.created_at DESC LIMIT $3 OFFSET $4",
    )
    .bind(user.tenant_id)
    .bind(&q.status)
    .bind(q.page.limit())
    .bind(q.page.offset())
    .fetch_all(&mut *tx)
    .await?;

    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sys.sys_audit_alert_delivery WHERE tenant_id = $1 AND ($2::text IS NULL OR status = $2)",
    )
    .bind(user.tenant_id)
    .bind(&q.status)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(Page {
        data: rows,
        page: PageMeta::new(q.page.page(), q.page.size(), total as u64),
    }))
}

/// 重投订阅死信。
#[utoipa::path(
    post,
    path = "/api/system/audit-deliveries/{id}/retry",
    tag = "system",
    params(("id" = Uuid, Path, description = "投递 id")),
    responses((status = 200, description = "已重置投递"))
)]
async fn retry_audit_delivery(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::AUDIT_ALERT)?;
    let result = sqlx::query(
        "UPDATE sys.sys_audit_alert_delivery
         SET status = 'pending', attempts = 0, next_attempt_at = NULL
         WHERE id = $1 AND tenant_id = $2 AND status = 'dead'",
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
        "system.audit.delivery.retry",
        "audit_delivery",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 批量重投当前租户全部投递死信。
#[utoipa::path(
    post,
    path = "/api/system/audit-deliveries/retry-dead",
    tag = "system",
    responses((status = 200, description = "重置数量", body = ApiResponse<u64>))
)]
async fn retry_all_dead_deliveries(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<u64>, AppError> {
    require_perm(&user, crate::perms::AUDIT_ALERT)?;
    let result = sqlx::query(
        "UPDATE sys.sys_audit_alert_delivery
         SET status = 'pending', attempts = 0, next_attempt_at = NULL
         WHERE tenant_id = $1 AND status = 'dead'",
    )
    .bind(user.tenant_id)
    .execute(&mut *tx)
    .await?;
    let count = result.rows_affected();
    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.audit.delivery.retry_all",
        "audit_delivery",
        None,
        Some(serde_json::json!({ "count": count })),
    )
    .await;
    Ok(ApiResponse::new(count))
}
