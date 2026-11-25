//! 租户用量接口：采集与趋势查询。
//!
//! 边界：用量数据表在 `tenant_usage` 领域模块；本模块只暴露 HTTP 接口。
//! 关键约束：趋势查询按租户隔离，走 TenantTx。

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::{require_perm, require_platform};
use crate::tenant::TenantTx;

/// 用量路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/tenants/usage/capture", post(capture_tenant_usage))
        .route("/tenants/{id}/usage", get(tenant_usage))
}

/// 租户用量查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantUsageQuery {
    /// 最近天数（默认 30）。
    pub days: Option<i64>,
}

/// 用量采集结果。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsageCaptureResult {
    /// 采集的租户数。
    pub captured: u64,
}

/// 采集所有租户当日用量。
#[utoipa::path(
    post,
    path = "/api/system/tenants/usage/capture",
    tag = "system",
    responses((status = 200, description = "采集结果", body = ApiResponse<UsageCaptureResult>))
)]
async fn capture_tenant_usage(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<ApiResponse<UsageCaptureResult>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_EDIT)?;
    let captured = crate::tenant_usage::capture_usage(&state.db)
        .await
        .map_err(|e| AppError::Internal(e.context("采集租户用量失败")))?;
    Ok(ApiResponse::new(UsageCaptureResult { captured }))
}

/// 查询租户用量趋势。
#[utoipa::path(
    get,
    path = "/api/system/tenants/{id}/usage",
    tag = "system",
    params(
        ("id" = Uuid, Path, description = "租户 id"),
        ("days" = Option<i64>, Query, description = "最近天数，默认 30")
    ),
    responses((status = 200, description = "用量趋势", body = ApiResponse<Vec<crate::tenant_usage::UsagePoint>>))
)]
async fn tenant_usage(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Query(q): Query<TenantUsageQuery>,
) -> Result<ApiResponse<Vec<crate::tenant_usage::UsagePoint>>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_LIST)?;
    let days = q.days.unwrap_or(30).clamp(1, 365);
    let rows = crate::tenant_usage::usage_history(&mut tx, id, days)
        .await
        .map_err(|e| AppError::Internal(e.context("查询租户用量失败")))?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}
