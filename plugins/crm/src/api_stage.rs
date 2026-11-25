//! CRM 商机阶段分析：阶段历史、停留时长、转化率与卡单。
//!
//! 边界：商机 CRUD 在 `api`；本模块只做只读分析。
//! 关键约束：全部查询按数据权限过滤，口径与列表页一致。

use axum::extract::{Query, State};
use kernel::prelude::*;
use serde::Serialize;
use sqlx::FromRow;

use crate::api::scope_filter;

/// 商机阶段历史记录。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StageHistoryVo {
    /// 变更前阶段（首次为空）。
    pub from_stage: Option<String>,
    /// 变更后阶段。
    pub to_stage: String,
    /// 变更时间。
    pub changed_at: chrono::DateTime<chrono::Utc>,
    /// 操作人。
    pub changed_by: Option<Uuid>,
}

/// 阶段平均停留时长。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StageDuration {
    /// 阶段。
    pub stage: String,
    /// 平均停留天数。
    pub avg_days: f64,
    /// 离开该阶段的次数。
    pub changes: i64,
}

/// 商机阶段历史。
#[utoipa::path(
    get,
    path = "/api/crm/opportunities/{id}/stage-history",
    tag = "crm",
    params(("id" = Uuid, Path, description = "商机 id")),
    responses((status = 200, description = "阶段历史", body = ApiResponse<Vec<StageHistoryVo>>))
)]
pub async fn opportunity_stage_history(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<Vec<StageHistoryVo>> {
    crate::perm::require(&user, crate::perm::OPPORTUNITY_LIST)?;
    let exists_sql = format!(
        "SELECT EXISTS(SELECT 1 FROM crm.opportunity c WHERE c.id = $1 AND c.deleted_at IS NULL {})",
        scope_filter("$2", "$3", "$4")
    );
    let visible: bool = sqlx::query_scalar(&exists_sql)
        .bind(id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_one(&mut *tx)
        .await?;
    if !visible {
        return Err(AppError::NotFound);
    }

    let rows = sqlx::query_as::<_, StageHistoryVo>(
        r#"
        SELECT from_stage, to_stage, changed_at, changed_by
        FROM crm.opportunity_stage_history
        WHERE opportunity_id = $1 AND tenant_id = $2
        ORDER BY changed_at ASC, id
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 阶段平均停留时长（销售流程卡点）。
#[utoipa::path(
    get,
    path = "/api/crm/opportunities/stage-duration",
    tag = "crm",
    responses((status = 200, description = "各阶段平均停留天数", body = ApiResponse<Vec<StageDuration>>))
)]
pub async fn opportunity_stage_duration(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> ApiResult<Vec<StageDuration>> {
    crate::perm::require(&user, crate::perm::OPPORTUNITY_LIST)?;
    let sql = format!(
        r#"
        SELECT h.from_stage AS stage,
               (avg(EXTRACT(EPOCH FROM (h.changed_at - h.prev))) / 86400.0)::float8 AS avg_days,
               count(*)::bigint AS changes
        FROM (
            SELECT h.*,
                   lag(h.changed_at) OVER (
                       PARTITION BY h.opportunity_id ORDER BY h.changed_at, h.id
                   ) AS prev
            FROM crm.opportunity_stage_history h
            WHERE h.tenant_id = $1
        ) h
        JOIN crm.opportunity c ON c.id = h.opportunity_id
        WHERE h.from_stage IS NOT NULL AND h.prev IS NOT NULL {}
        GROUP BY h.from_stage
        "#,
        scope_filter("$2", "$3", "$4")
    );
    let rows = sqlx::query_as::<_, StageDuration>(&sql)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 阶段转化（from → to 的次数与占比）。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StageConversion {
    /// 来源阶段。
    pub from_stage: String,
    /// 目标阶段。
    pub to_stage: String,
    /// 转化次数。
    pub count: i64,
    /// 占该来源阶段全部转出的比例（%）。
    pub percent: f64,
}

/// 阶段转化率。
#[utoipa::path(
    get,
    path = "/api/crm/opportunities/stage-conversion",
    tag = "crm",
    responses((status = 200, description = "阶段转化统计", body = ApiResponse<Vec<StageConversion>>))
)]
pub async fn opportunity_stage_conversion(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> ApiResult<Vec<StageConversion>> {
    crate::perm::require(&user, crate::perm::OPPORTUNITY_LIST)?;
    let sql = format!(
        r#"
        SELECT COALESCE(h.from_stage, '') AS from_stage,
               h.to_stage,
               count(*)::bigint AS count,
               (100.0 * count(*) / sum(count(*)) OVER (PARTITION BY h.from_stage))::float8 AS percent
        FROM crm.opportunity_stage_history h
        JOIN crm.opportunity c ON c.id = h.opportunity_id
        WHERE h.tenant_id = $1 AND h.from_stage IS NOT NULL {}
        GROUP BY h.from_stage, h.to_stage
        ORDER BY h.from_stage, count DESC
        "#,
        scope_filter("$2", "$3", "$4")
    );
    let rows = sqlx::query_as::<_, StageConversion>(&sql)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 停留超时的商机。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StuckOpportunity {
    /// 商机 id。
    pub opportunity_id: Uuid,
    /// 商机名称。
    pub name: String,
    /// 客户名称。
    pub customer_name: String,
    /// 当前阶段。
    pub stage: String,
    /// 金额（分）。
    pub amount_cents: Option<i32>,
    /// 在当前阶段停留天数。
    pub days_in_stage: f64,
}

/// 停留超时查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StuckQuery {
    /// 阈值天数，默认 14，最大 365。
    #[serde(default)]
    pub days: Option<i64>,
}

/// 停留超时的商机（当前阶段超过 N 天，且未赢单/输单）。
#[utoipa::path(
    get,
    path = "/api/crm/opportunities/stuck",
    tag = "crm",
    params(("days" = Option<i64>, Query, description = "阈值天数，默认 14")),
    responses((status = 200, description = "超时商机列表", body = ApiResponse<Vec<StuckOpportunity>>))
)]
pub async fn stuck_opportunities(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<StuckQuery>,
) -> ApiResult<Vec<StuckOpportunity>> {
    crate::perm::require(&user, crate::perm::OPPORTUNITY_LIST)?;
    let days = q.days.unwrap_or(14).clamp(1, 365);
    let sql = format!(
        r#"
        WITH last_change AS (
            SELECT DISTINCT ON (opportunity_id)
                   opportunity_id, to_stage, changed_at
            FROM crm.opportunity_stage_history
            WHERE tenant_id = $1
            ORDER BY opportunity_id, changed_at DESC, id DESC
        )
        SELECT c.id AS opportunity_id,
               c.name,
               cu.name AS customer_name,
               c.stage,
               c.amount_cents,
               (EXTRACT(EPOCH FROM (now() - l.changed_at)) / 86400.0)::float8 AS days_in_stage
        FROM last_change l
        JOIN crm.opportunity c ON c.id = l.opportunity_id
        JOIN crm.customer cu ON cu.id = c.customer_id
        WHERE c.deleted_at IS NULL AND c.tenant_id = $1
          AND c.stage NOT IN ('won', 'lost')
          AND l.changed_at < now() - make_interval(days => $2::int)
          {}
        ORDER BY days_in_stage DESC
        LIMIT 50
        "#,
        scope_filter("$3", "$4", "$5")
    );
    let rows = sqlx::query_as::<_, StuckOpportunity>(&sql)
        .bind(user.tenant_id)
        .bind(days as i32)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}
