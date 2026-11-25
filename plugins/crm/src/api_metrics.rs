//! CRM 统计：商机阶段分布与合同收款流水。
//!
//! 边界：阶段历史/转化在 `api_stage`；本模块只做聚合查询。
//! 关键约束：按数据权限过滤；金额统一用分。

use axum::extract::{Query, State};
use kernel::prelude::*;
use serde::Serialize;
use sqlx::FromRow;

use crate::api::scope_filter;

/// 商机阶段统计。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StageStat {
    /// 阶段。
    pub stage: String,
    /// 数量。
    pub count: i64,
    /// 金额合计（分）。
    pub amount_cents: i64,
}

/// 商机漏斗：按阶段汇总数量与金额（受数据权限约束）。
#[utoipa::path(
    get,
    path = "/api/crm/opportunities/stats",
    tag = "crm",
    responses((status = 200, description = "各阶段汇总", body = ApiResponse<Vec<StageStat>>))
)]
pub async fn opportunity_stats(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> ApiResult<Vec<StageStat>> {
    crate::perm::require(&user, crate::perm::OPPORTUNITY_LIST)?;
    let sql = format!(
        r#"
        SELECT c.stage,
               count(*)::bigint AS count,
               COALESCE(sum(c.amount_cents), 0)::bigint AS amount_cents
        FROM crm.opportunity c
        WHERE c.deleted_at IS NULL AND c.tenant_id = $1
          {}
        GROUP BY c.stage
        "#,
        scope_filter("$2", "$3", "$4")
    );

    let rows = sqlx::query_as::<_, StageStat>(&sql)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_all(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 合同回款汇总。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContractPaymentsVo {
    /// 合同金额（分）。
    pub contract_amount_cents: i64,
    /// 已回款（分）。
    pub paid_cents: i64,
    /// 未回款（分）。
    pub remaining_cents: i64,
    /// 回款明细。
    pub items: Vec<crate::payment::PaymentVo>,
    /// 明细总条数。
    pub total: i64,
    /// 当前页码（从 1 开始）。
    pub page: u32,
    /// 每页条数。
    pub size: u32,
}

/// 回款明细分页参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaymentQuery {
    /// 页码（默认 1）。
    #[serde(default)]
    pub page: Option<u32>,
    /// 每页条数（默认 50，最大 200）。
    #[serde(default)]
    pub size: Option<u32>,
}

/// 合同下的回款明细与汇总。
#[utoipa::path(
    get,
    path = "/api/crm/contracts/{id}/payments",
    tag = "crm",
    params(("id" = Uuid, Path, description = "合同 id")),
    responses((status = 200, description = "回款汇总", body = ApiResponse<ContractPaymentsVo>))
)]
pub async fn contract_payments(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Query(q): Query<PaymentQuery>,
) -> ApiResult<ContractPaymentsVo> {
    crate::perm::require(&user, crate::perm::PAYMENT_LIST)?;
    let sql = format!(
        r#"
        SELECT c.amount_cents, c.paid_cents
        FROM crm.contract c
        WHERE c.id = $1 AND c.deleted_at IS NULL {}
        "#,
        scope_filter("$2", "$3", "$4")
    );
    let contract_row: Option<(i32, i64)> = sqlx::query_as(&sql)
        .bind(id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_optional(&mut *tx)
        .await?;
    // paid_cents 由触发器维护，这里不再对回款表做 sum()
    let (contract_amount, paid_cents) = contract_row.ok_or(AppError::NotFound)?;

    // 真分页：默认 50 条，最大 200 条，避免响应体随数据量线性膨胀
    let page = q.page.unwrap_or(1).max(1);
    let size = q.size.unwrap_or(50).clamp(1, 200);
    let offset = i64::from(page - 1) * i64::from(size);

    let total: i64 = sqlx::query_scalar(
        r#"
        SELECT count(*)::bigint
        FROM crm.payment
        WHERE contract_id = $1 AND tenant_id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_one(&mut *tx)
    .await?;

    let items = sqlx::query_as::<_, crate::payment::PaymentVo>(
        r#"
        SELECT id, contract_id, payment_no, amount_cents, paid_at, method,
               owner_id, dept_id, created_at, updated_at
        FROM crm.payment
        WHERE contract_id = $1 AND tenant_id = $2 AND deleted_at IS NULL
        ORDER BY paid_at DESC NULLS LAST, created_at DESC
        LIMIT $3 OFFSET $4
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(i64::from(size))
    .bind(offset)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(ApiResponse::new(ContractPaymentsVo {
        contract_amount_cents: contract_amount as i64,
        paid_cents,
        remaining_cents: contract_amount as i64 - paid_cents,
        items,
        total,
        page,
        size,
    }))
}
