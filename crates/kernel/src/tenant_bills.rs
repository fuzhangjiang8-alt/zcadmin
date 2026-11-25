//! 租户账单：生成、列表、状态变更与对账。
//!
//! 边界：计费配置/预估在 `tenant_billing`；回填在 `tenant_bill_backfill`；
//! 收款/退款在 `tenant_bill_payments`；Excel 在 `tenant_bill_export`。
//! 关键约束：账单按月生成；状态变更需满足状态机。

use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::{require_perm, require_platform};
use crate::tenant::TenantTx;

/// 账单路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/tenants/bills/generate", post(generate_tenant_bills))
        .route("/tenants/bills", get(list_tenant_bills))
        .route("/tenants/bills/reconciliation", get(bill_reconciliation))
        .route("/tenants/bills/{id}/status", put(update_bill_status))
}

/// 租户用量查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantBillQuery {
    /// 月份 YYYY-MM。
    pub month: Option<String>,
}

/// 租户账单记录。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantBillRow {
    /// 账单 id。
    pub id: Uuid,
    /// 租户 id。
    pub tenant_id: Uuid,
    /// 租户编码。
    pub tenant_code: String,
    /// 租户名称。
    pub tenant_name: String,
    /// 账单月份。
    pub bill_month: chrono::NaiveDate,
    /// 币种。
    pub currency: String,
    /// 平均用户数。
    pub avg_users: f64,
    /// 平均存储字节。
    pub avg_storage_bytes: i64,
    /// 用户费用（分）。
    pub user_amount_cents: i64,
    /// 存储费用（分）。
    pub storage_amount_cents: i64,
    /// 合计（分）。
    pub total_cents: i64,
    /// 已收款（分）。
    pub paid_cents: i64,
    /// 状态：draft / issued / paid / void。
    pub status: String,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 账单生成结果。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BillGenerateResult {
    /// 月份。
    pub month: String,
    /// 生成/更新的账单数。
    pub generated: u64,
}

/// 生成指定月份租户账单（幂等，重复生成覆盖草稿）。
#[utoipa::path(
    post,
    path = "/api/system/tenants/bills/generate",
    tag = "system",
    params(("month" = Option<String>, Query, description = "YYYY-MM，默认当月")),
    responses((status = 200, description = "生成结果", body = ApiResponse<BillGenerateResult>))
)]
async fn generate_tenant_bills(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<TenantBillQuery>,
) -> Result<ApiResponse<BillGenerateResult>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_EDIT)?;
    let month = q
        .month
        .filter(|m| m.len() == 7)
        .unwrap_or_else(|| Utc::now().format("%Y-%m").to_string());
    let generated = crate::tenant_usage::generate_bills(&state.db, &month)
        .await
        .map_err(|e| AppError::Internal(e.context("生成账单失败")))?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.tenant.bills.generate",
        "sys_tenant_bill",
        None,
        Some(serde_json::json!({ "month": month, "generated": generated })),
    )
    .await;

    Ok(ApiResponse::new(BillGenerateResult { month, generated }))
}

/// 查询租户账单列表。
#[utoipa::path(
    get,
    path = "/api/system/tenants/bills",
    tag = "system",
    params(("month" = Option<String>, Query, description = "YYYY-MM，不传返回全部")),
    responses((status = 200, description = "账单列表", body = ApiResponse<Vec<TenantBillRow>>))
)]
async fn list_tenant_bills(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<TenantBillQuery>,
) -> Result<ApiResponse<Vec<TenantBillRow>>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_LIST)?;
    let month = q
        .month
        .filter(|m| m.len() == 7)
        .and_then(|m| chrono::NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d").ok());
    let rows: Vec<TenantBillRow> = sqlx::query_as(
        r#"
        SELECT b.id, b.tenant_id, t.code AS tenant_code, t.name AS tenant_name,
               b.bill_month, b.currency, b.avg_users::float8 AS avg_users,
               b.avg_storage_bytes, b.user_amount_cents, b.storage_amount_cents,
               b.total_cents,
               COALESCE((SELECT sum(CASE WHEN p.kind = 'refund' THEN -p.amount_cents ELSE p.amount_cents END)
                         FROM sys.sys_tenant_bill_payment p WHERE p.bill_id = b.id), 0)::bigint AS paid_cents,
               b.status, b.created_at
        FROM sys.sys_tenant_bill b
        JOIN sys.sys_tenant t ON t.id = b.tenant_id
        WHERE ($1::date IS NULL OR b.bill_month = $1)
        ORDER BY b.bill_month DESC, t.code
        "#,
    )
    .bind(month)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 账单状态变更请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BillStatusRequest {
    /// 目标状态：issued / paid / void。
    pub status: String,
}

/// 变更账单状态。
#[utoipa::path(
    put,
    path = "/api/system/tenants/bills/{id}/status",
    tag = "system",
    params(("id" = Uuid, Path, description = "账单 id")),
    request_body = BillStatusRequest,
    responses((status = 200, description = "已更新"))
)]
async fn update_bill_status(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<BillStatusRequest>,
) -> Result<ApiResponse<()>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_EDIT)?;
    let current: Option<String> =
        sqlx::query_scalar("SELECT status FROM sys.sys_tenant_bill WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(current) = current else {
        return Err(AppError::NotFound);
    };
    let allowed = matches!(
        (current.as_str(), req.status.as_str()),
        ("draft", "issued") | ("draft", "void") | ("issued", "paid") | ("issued", "void")
    );
    if !allowed {
        return Err(AppError::Validation(format!(
            "账单状态不允许从 {current} 变更为 {}",
            req.status
        )));
    }
    sqlx::query("UPDATE sys.sys_tenant_bill SET status = $1, updated_at = now() WHERE id = $2")
        .bind(&req.status)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.tenant.bill.status",
        "sys_tenant_bill",
        Some(&id.to_string()),
        Some(serde_json::json!({ "from": current, "to": req.status })),
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 账单对账行。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReconciliationRow {
    /// 租户 id。
    pub tenant_id: Uuid,
    /// 租户编码。
    pub tenant_code: String,
    /// 租户名称。
    pub tenant_name: String,
    /// 账单数。
    pub bill_count: i64,
    /// 应收（分）。
    pub total_cents: i64,
    /// 收款（分）。
    pub paid_cents: i64,
    /// 退款（分）。
    pub refunded_cents: i64,
    /// 净收款（分）。
    pub net_cents: i64,
    /// 未收（分）。
    pub outstanding_cents: i64,
}

/// 账单对账汇总。
#[utoipa::path(
    get,
    path = "/api/system/tenants/bills/reconciliation",
    tag = "system",
    params(("month" = Option<String>, Query, description = "YYYY-MM，不传返回全部")),
    responses((status = 200, description = "对账汇总", body = ApiResponse<Vec<ReconciliationRow>>))
)]
async fn bill_reconciliation(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<TenantBillQuery>,
) -> Result<ApiResponse<Vec<ReconciliationRow>>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_LIST)?;
    let month = q
        .month
        .filter(|m| m.len() == 7)
        .and_then(|m| chrono::NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d").ok());
    let rows: Vec<ReconciliationRow> = sqlx::query_as(
        r#"
        WITH bills AS (
            SELECT * FROM sys.sys_tenant_bill
            WHERE ($1::date IS NULL OR bill_month = $1)
        ),
        pay AS (
            SELECT bill_id,
                   COALESCE(sum(CASE WHEN kind = 'payment' THEN amount_cents ELSE 0 END), 0)::bigint AS paid,
                   COALESCE(sum(CASE WHEN kind = 'refund' THEN amount_cents ELSE 0 END), 0)::bigint AS refunded
            FROM sys.sys_tenant_bill_payment
            GROUP BY bill_id
        )
        SELECT t.id AS tenant_id, t.code AS tenant_code, t.name AS tenant_name,
               count(b.id)::bigint AS bill_count,
               COALESCE(sum(b.total_cents), 0)::bigint AS total_cents,
               COALESCE(sum(pay.paid), 0)::bigint AS paid_cents,
               COALESCE(sum(pay.refunded), 0)::bigint AS refunded_cents,
               COALESCE(sum(pay.paid - pay.refunded), 0)::bigint AS net_cents,
               COALESCE(sum(b.total_cents - (COALESCE(pay.paid, 0) - COALESCE(pay.refunded, 0))), 0)::bigint AS outstanding_cents
        FROM sys.sys_tenant t
        LEFT JOIN bills b ON b.tenant_id = t.id
        LEFT JOIN pay ON pay.bill_id = b.id
        WHERE t.deleted_at IS NULL
        GROUP BY t.id, t.code, t.name
        ORDER BY t.code
        "#,
    )
    .bind(month)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}
