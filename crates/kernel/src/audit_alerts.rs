//! 审计告警列表与确认。
//!
//! 边界：告警规则/配置/投递在其它审计模块；本模块只处理告警记录。
//! 关键约束：按租户隔离；确认操作需 `system:audit:alert` 权限。

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::{ApiResponse, Page, PageMeta, PageQuery};
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;

/// 审计告警路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/audit-alerts", get(list_audit_alerts))
        .route("/audit-alerts/{id}/ack", post(ack_audit_alert))
        .route("/audit-alerts/subscribed", get(list_subscribed_alerts))
}

/// 审计告警视图。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertVo {
    /// 告警 id。
    pub id: Uuid,
    /// 动作。
    pub action: String,
    /// 窗口开始。
    pub window_start: DateTime<Utc>,
    /// 窗口结束。
    pub window_end: DateTime<Utc>,
    /// 次数。
    pub count: i64,
    /// 阈值。
    pub threshold: i32,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 处理时间。
    pub acknowledged_at: Option<DateTime<Utc>>,
    /// 处理人。
    pub acknowledged_by: Option<Uuid>,
    /// Webhook 死信时间。
    pub notify_failed_at: Option<DateTime<Utc>>,
    /// 关联用户 id。
    pub user_id: Option<Uuid>,
    /// 关联用户名。
    pub username: Option<String>,
}

/// 告警查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditAlertQuery {
    /// 分页参数。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 只看未处理（默认 true）。
    pub unacked: Option<bool>,
}

/// 审计告警列表。
#[utoipa::path(
    get,
    path = "/api/system/audit-alerts",
    tag = "system",
    responses((status = 200, description = "告警分页", body = ApiResponse<Page<AlertVo>>))
)]
async fn list_audit_alerts(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<AuditAlertQuery>,
) -> Result<ApiResponse<Page<AlertVo>>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let unacked = q.unacked.unwrap_or(true);

    let rows: Vec<AlertVo> = sqlx::query_as(
        r#"
        SELECT a.id, a.action, a.window_start, a.window_end, a.count, a.threshold,
               a.created_at, a.acknowledged_at, a.acknowledged_by, a.notify_failed_at,
               a.user_id, u.username
        FROM sys.sys_audit_alert a
        LEFT JOIN sys.sys_user u ON u.id = a.user_id
        WHERE a.tenant_id = $1 AND (NOT $2::bool OR a.acknowledged_at IS NULL)
        ORDER BY created_at DESC
        LIMIT $3 OFFSET $4
        "#,
    )
    .bind(user.tenant_id)
    .bind(unacked)
    .bind(q.page.limit())
    .bind(q.page.offset())
    .fetch_all(&mut *tx)
    .await?;

    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sys.sys_audit_alert WHERE tenant_id = $1 AND (NOT $2::bool OR acknowledged_at IS NULL)",
    )
    .bind(user.tenant_id)
    .bind(unacked)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(Page {
        data: rows,
        page: PageMeta::new(q.page.page(), q.page.size(), total as u64),
    }))
}

/// 处理审计告警。
#[utoipa::path(
    post,
    path = "/api/system/audit-alerts/{id}/ack",
    tag = "system",
    params(("id" = Uuid, Path, description = "告警 id")),
    responses((status = 200, description = "已处理"))
)]
async fn ack_audit_alert(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::AUDIT_ALERT)?;
    let result = sqlx::query(
        "UPDATE sys.sys_audit_alert SET acknowledged_at = now(), acknowledged_by = $3
         WHERE id = $1 AND tenant_id = $2 AND acknowledged_at IS NULL",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.audit.alert.ack",
        "audit_alert",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 当前用户订阅的告警（最近 30 天，未处理）。
#[utoipa::path(
    get,
    path = "/api/system/audit-alerts/subscribed",
    tag = "system",
    responses((status = 200, description = "订阅告警分页", body = ApiResponse<Page<AlertVo>>))
)]
pub(crate) async fn list_subscribed_alerts(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<PageQuery>,
) -> Result<ApiResponse<Page<AlertVo>>, AppError> {
    let like_expr = crate::audit_subscriptions::like_expression("s");
    let rows: Vec<AlertVo> = sqlx::query_as(&format!(
        r#"
        SELECT a.id, a.action, a.window_start, a.window_end, a.count, a.threshold,
               a.created_at, a.acknowledged_at, a.acknowledged_by, a.notify_failed_at,
               a.user_id, u.username
        FROM sys.sys_audit_alert a
        LEFT JOIN sys.sys_user u ON u.id = a.user_id
        WHERE a.tenant_id = $1
          AND a.acknowledged_at IS NULL
          AND a.created_at >= now() - interval '30 days'
          AND EXISTS (
              SELECT 1 FROM sys.sys_audit_alert_subscription s
              WHERE s.tenant_id = $1 AND s.user_id = $2
                AND a.action LIKE {like_expr} ESCAPE '\'
          )
        ORDER BY a.created_at DESC
        LIMIT $3 OFFSET $4
        "#
    ))
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(q.limit())
    .bind(q.offset())
    .fetch_all(&mut *tx)
    .await?;

    let total: i64 = sqlx::query_scalar(&format!(
        r#"
        SELECT count(*) FROM sys.sys_audit_alert a
        WHERE a.tenant_id = $1 AND a.acknowledged_at IS NULL
          AND a.created_at >= now() - interval '30 days'
          AND EXISTS (
              SELECT 1 FROM sys.sys_audit_alert_subscription s
              WHERE s.tenant_id = $1 AND s.user_id = $2
                AND a.action LIKE {like_expr} ESCAPE '\'
          )
        "#
    ))
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(Page {
        data: rows,
        page: PageMeta::new(q.page(), q.size(), total as u64),
    }))
}
