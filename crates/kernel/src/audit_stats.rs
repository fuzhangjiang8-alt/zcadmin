//! 审计统计接口：总量、按用户、按动作、按天。
//!
//! 边界：告警/投递/配置在其它审计模块；本模块只读聚合。
//! 关键约束：按租户隔离；时间范围默认 7 天。

use axum::extract::{Query, State};
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;

/// 审计统计路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/audit-logs/stats", get(audit_stats))
}

/// 审计统计。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditStatsVo {
    /// 统计天数。
    pub days: i64,
    /// 区间总条数。
    pub total: i64,
    /// 按动作计数（Top 20）。
    pub by_action: Vec<ActionCountVo>,
    /// 按天计数。
    pub by_day: Vec<DayCountVo>,
}

/// 按用户的告警计数。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlertUserStat {
    /// 用户名。
    pub username: String,
    /// 数量。
    pub count: i64,
}

/// 动作计数。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActionCountVo {
    /// 动作。
    pub action: String,
    /// 次数。
    pub count: i64,
}

/// 日期计数。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DayCountVo {
    /// 日期（YYYY-MM-DD）。
    pub day: String,
    /// 次数。
    pub count: i64,
}

/// 审计统计查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditStatsQuery {
    /// 最近天数，默认 7，最大 90。
    pub days: Option<i64>,
}

/// 审计统计：按动作与按天计数。
#[utoipa::path(
    get,
    path = "/api/system/audit-logs/stats",
    tag = "system",
    params(("days" = Option<i64>, Query, description = "最近天数，默认 7")),
    responses((status = 200, description = "审计统计", body = ApiResponse<AuditStatsVo>))
)]
async fn audit_stats(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<AuditStatsQuery>,
) -> Result<ApiResponse<AuditStatsVo>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let days = q.days.unwrap_or(7).clamp(1, 90);

    let by_action: Vec<ActionCountVo> = sqlx::query_as(
        r#"
        SELECT action, count(*)::bigint AS count
        FROM sys.sys_audit_log
        WHERE tenant_id = $1 AND created_at >= now() - make_interval(days => $2::int)
        GROUP BY action
        ORDER BY count DESC, action
        LIMIT 20
        "#,
    )
    .bind(user.tenant_id)
    .bind(days as i32)
    .fetch_all(&mut *tx)
    .await?;

    let by_day: Vec<DayCountVo> = sqlx::query_as(
        r#"
        SELECT to_char(created_at::date, 'YYYY-MM-DD') AS day, count(*)::bigint AS count
        FROM sys.sys_audit_log
        WHERE tenant_id = $1 AND created_at >= now() - make_interval(days => $2::int)
        GROUP BY created_at::date
        ORDER BY created_at::date
        "#,
    )
    .bind(user.tenant_id)
    .bind(days as i32)
    .fetch_all(&mut *tx)
    .await?;

    let total: i64 = by_day.iter().map(|d| d.count).sum();
    tx.commit().await?;
    Ok(ApiResponse::new(AuditStatsVo {
        days,
        total,
        by_action,
        by_day,
    }))
}
