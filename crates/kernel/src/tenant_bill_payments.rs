//! 账单收款与退款接口。
//!
//! 边界：账单主体/状态在 `tenant_admin`；对账与催收在各自模块。
//! 关键约束：收款/退款都写流水表，并在收满时自动把账单置为 paid。

use axum::extract::{Path, State};
use axum::routing::{get, post};
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

/// 收款/退款路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/tenants/bills/{id}/payments",
            get(list_bill_payments).post(create_bill_payment),
        )
        .route("/tenants/bills/{id}/refunds", post(create_bill_refund))
}

/// 收款流水。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BillPaymentVo {
    /// 流水 id。
    pub id: Uuid,
    /// 账单 id。
    pub bill_id: Uuid,
    /// 金额（分）。
    pub amount_cents: i64,
    /// 类型：payment / refund。
    pub kind: String,
    /// 方式。
    pub method: String,
    /// 外部凭证号。
    pub reference: Option<String>,
    /// 备注。
    pub note: Option<String>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 收款请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BillPaymentRequest {
    /// 金额（分）。
    pub amount_cents: i64,
    /// 方式：bank / cash / other。
    pub method: String,
    /// 外部凭证号。
    pub reference: Option<String>,
    /// 备注。
    pub note: Option<String>,
}

/// 查询账单收款流水。
#[utoipa::path(
    get,
    path = "/api/system/tenants/bills/{id}/payments",
    tag = "system",
    params(("id" = Uuid, Path, description = "账单 id")),
    responses((status = 200, description = "收款流水", body = ApiResponse<Vec<BillPaymentVo>>))
)]
async fn list_bill_payments(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<Vec<BillPaymentVo>>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_LIST)?;
    let rows: Vec<BillPaymentVo> = sqlx::query_as(
        "SELECT id, bill_id, amount_cents, kind, method, reference, note, created_at
         FROM sys.sys_tenant_bill_payment WHERE bill_id = $1 ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    Ok(ApiResponse::new(rows))
}

/// 登记收款；收满后账单自动置为 paid。
#[utoipa::path(
    post,
    path = "/api/system/tenants/bills/{id}/payments",
    tag = "system",
    params(("id" = Uuid, Path, description = "账单 id")),
    request_body = BillPaymentRequest,
    responses((status = 200, description = "已登记", body = ApiResponse<BillPaymentVo>))
)]
async fn create_bill_payment(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<BillPaymentRequest>,
) -> Result<ApiResponse<BillPaymentVo>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_EDIT)?;
    if req.amount_cents <= 0 {
        return Err(AppError::Validation("收款金额必须大于 0".into()));
    }
    if req.method.trim().is_empty() {
        return Err(AppError::Validation("收款方式不能为空".into()));
    }
    let bill: Option<(String, i64, Uuid)> = sqlx::query_as(
        "SELECT status, total_cents, tenant_id FROM sys.sys_tenant_bill WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((status, total_cents, tenant_id)) = bill else {
        return Err(AppError::NotFound);
    };
    if status != "issued" {
        return Err(AppError::Validation("只有已出账账单可以登记收款".into()));
    }
    let paid: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(CASE WHEN kind = 'refund' THEN -amount_cents ELSE amount_cents END), 0)::bigint
         FROM sys.sys_tenant_bill_payment WHERE bill_id = $1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if paid + req.amount_cents > total_cents {
        return Err(AppError::Validation("收款金额超过未收余额".into()));
    }
    let vo: BillPaymentVo = sqlx::query_as(
        "INSERT INTO sys.sys_tenant_bill_payment
             (id, bill_id, tenant_id, amount_cents, method, reference, note, created_by)
         VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6, $7)
         RETURNING id, bill_id, amount_cents, kind, method, reference, note, created_at",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(req.amount_cents)
    .bind(req.method.trim())
    .bind(&req.reference)
    .bind(&req.note)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;
    if paid + req.amount_cents >= total_cents {
        sqlx::query(
            "UPDATE sys.sys_tenant_bill SET status = 'paid', updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.tenant.bill.payment",
        "sys_tenant_bill",
        Some(&id.to_string()),
        Some(serde_json::json!({
            "amountCents": req.amount_cents,
            "method": req.method,
            "reference": req.reference
        })),
    )
    .await;
    Ok(ApiResponse::new(vo))
}

/// 退款请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BillRefundRequest {
    /// 退款金额（分）。
    pub amount_cents: i64,
    /// 退款原因。
    pub reason: String,
}

/// 登记退款/冲正。
#[utoipa::path(
    post,
    path = "/api/system/tenants/bills/{id}/refunds",
    tag = "system",
    params(("id" = Uuid, Path, description = "账单 id")),
    request_body = BillRefundRequest,
    responses((status = 200, description = "已退款", body = ApiResponse<BillPaymentVo>))
)]
async fn create_bill_refund(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<BillRefundRequest>,
) -> Result<ApiResponse<BillPaymentVo>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_EDIT)?;
    if req.amount_cents <= 0 {
        return Err(AppError::Validation("退款金额必须大于 0".into()));
    }
    if req.reason.trim().is_empty() {
        return Err(AppError::Validation("退款原因不能为空".into()));
    }
    let bill: Option<(String, i64, Uuid)> = sqlx::query_as(
        "SELECT status, total_cents, tenant_id FROM sys.sys_tenant_bill WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((status, total_cents, tenant_id)) = bill else {
        return Err(AppError::NotFound);
    };
    if status != "issued" && status != "paid" {
        return Err(AppError::Validation("只有已出账/已收款账单可以退款".into()));
    }
    let paid: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(CASE WHEN kind = 'refund' THEN -amount_cents ELSE amount_cents END), 0)::bigint
         FROM sys.sys_tenant_bill_payment WHERE bill_id = $1",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if req.amount_cents > paid {
        return Err(AppError::Validation("退款金额超过已收余额".into()));
    }
    let vo: BillPaymentVo = sqlx::query_as(
        "INSERT INTO sys.sys_tenant_bill_payment
             (id, bill_id, tenant_id, amount_cents, kind, method, reference, note, created_by)
         VALUES (gen_random_uuid(), $1, $2, $3, 'refund', 'refund', NULL, $4, $5)
         RETURNING id, bill_id, amount_cents, kind, method, reference, note, created_at",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(req.amount_cents)
    .bind(req.reason.trim())
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;
    if status == "paid" && paid - req.amount_cents < total_cents {
        sqlx::query(
            "UPDATE sys.sys_tenant_bill SET status = 'issued', updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.tenant.bill.refund",
        "sys_tenant_bill",
        Some(&id.to_string()),
        Some(serde_json::json!({
            "amountCents": req.amount_cents,
            "reason": req.reason
        })),
    )
    .await;
    Ok(ApiResponse::new(vo))
}
