//! 账单回填：按历史用量补齐账单。
//!
//! 边界：账单生成/列表在 `tenant_bills`；本模块只做回填。
//! 关键约束：幂等，已存在账单的月份跳过。

use axum::extract::{Query, State};
use axum::routing::post;
use axum::Router;
use serde::{Deserialize, Serialize};

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::{require_perm, require_platform};

/// 回填路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/tenants/bills/backfill", post(backfill_tenant_bills))
}

/// 账单补生成参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BillBackfillQuery {
    /// 起始月份 YYYY-MM。
    pub from: String,
    /// 结束月份 YYYY-MM。
    pub to: String,
}

/// 补生成结果。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BillBackfillResult {
    /// 处理月份数。
    pub months: u64,
    /// 生成/更新账单数。
    pub bills: u64,
}

/// 补生成指定月份区间的账单。
#[utoipa::path(
    post,
    path = "/api/system/tenants/bills/backfill",
    tag = "system",
    params(
        ("from" = String, Query, description = "起始月份 YYYY-MM"),
        ("to" = String, Query, description = "结束月份 YYYY-MM")
    ),
    responses((status = 200, description = "补生成结果", body = ApiResponse<BillBackfillResult>))
)]
async fn backfill_tenant_bills(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<BillBackfillQuery>,
) -> Result<ApiResponse<BillBackfillResult>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_EDIT)?;
    let mut current = chrono::NaiveDate::parse_from_str(&format!("{}-01", q.from), "%Y-%m-%d")
        .map_err(|_| AppError::Validation("起始月份格式必须为 YYYY-MM".into()))?;
    let end = chrono::NaiveDate::parse_from_str(&format!("{}-01", q.to), "%Y-%m-%d")
        .map_err(|_| AppError::Validation("结束月份格式必须为 YYYY-MM".into()))?;
    if current > end {
        return Err(AppError::Validation("起始月份不能晚于结束月份".into()));
    }
    if (end - current).num_days() > 366 * 5 {
        return Err(AppError::Validation("补生成区间不能超过 5 年".into()));
    }
    let mut months = 0;
    let mut bills = 0;
    while current <= end {
        let month = current.format("%Y-%m").to_string();
        bills += crate::tenant_usage::generate_bills(&state.db, &month)
            .await
            .map_err(|e| AppError::Internal(e.context("补生成账单失败")))?;
        months += 1;
        current = current
            .checked_add_months(chrono::Months::new(1))
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("月份溢出")))?;
    }
    Ok(ApiResponse::new(BillBackfillResult { months, bills }))
}
