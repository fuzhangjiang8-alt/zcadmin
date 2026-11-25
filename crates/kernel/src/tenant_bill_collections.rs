//! 账单催收记录与催收承诺提醒扫描。
//!
//! 边界：账单主体在 `tenant_admin`；收款/退款在 `tenant_bill_payments`。
//! 关键约束：扫描按租户循环设置 RLS 上下文；提醒 7 天内不重复。

use axum::extract::{Path, Query, State};
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
use crate::tenant_usage_api::TenantUsageQuery;

/// 催收路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/tenants/bills/{id}/collections",
            get(list_bill_collections).post(create_bill_collection),
        )
        .route(
            "/tenants/bills/collections/reminders/scan",
            post(scan_collection_reminders_handler),
        )
}

/// 催收记录。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BillCollectionVo {
    /// 记录 id。
    pub id: Uuid,
    /// 账单 id。
    pub bill_id: Uuid,
    /// 催收备注。
    pub note: String,
    /// 承诺还款金额（分）。
    pub promised_amount_cents: Option<i64>,
    /// 承诺还款日期。
    pub promised_date: Option<chrono::NaiveDate>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 催收请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BillCollectionRequest {
    /// 催收备注。
    pub note: String,
    /// 承诺还款金额（分）。
    pub promised_amount_cents: Option<i64>,
    /// 承诺还款日期。
    pub promised_date: Option<chrono::NaiveDate>,
}

/// 查询催收记录。
#[utoipa::path(
    get,
    path = "/api/system/tenants/bills/{id}/collections",
    tag = "system",
    params(("id" = Uuid, Path, description = "账单 id")),
    responses((status = 200, description = "催收记录", body = ApiResponse<Vec<BillCollectionVo>>))
)]
async fn list_bill_collections(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<Vec<BillCollectionVo>>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_LIST)?;
    let rows: Vec<BillCollectionVo> = sqlx::query_as(
        "SELECT id, bill_id, note, promised_amount_cents, promised_date, created_at
         FROM sys.sys_tenant_bill_collection WHERE bill_id = $1 ORDER BY created_at DESC",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    Ok(ApiResponse::new(rows))
}

/// 新增催收记录。
#[utoipa::path(
    post,
    path = "/api/system/tenants/bills/{id}/collections",
    tag = "system",
    params(("id" = Uuid, Path, description = "账单 id")),
    request_body = BillCollectionRequest,
    responses((status = 200, description = "已记录", body = ApiResponse<BillCollectionVo>))
)]
async fn create_bill_collection(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<BillCollectionRequest>,
) -> Result<ApiResponse<BillCollectionVo>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_EDIT)?;
    if req.note.trim().is_empty() {
        return Err(AppError::Validation("催收备注不能为空".into()));
    }
    let bill: Option<(String, Uuid)> =
        sqlx::query_as("SELECT status, tenant_id FROM sys.sys_tenant_bill WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((status, tenant_id)) = bill else {
        return Err(AppError::NotFound);
    };
    if status != "issued" && status != "paid" {
        return Err(AppError::Validation("当前账单状态不可催收".into()));
    }
    if req.promised_amount_cents.unwrap_or(0) < 0 {
        return Err(AppError::Validation("承诺金额不能为负".into()));
    }
    let vo: BillCollectionVo = sqlx::query_as(
        "INSERT INTO sys.sys_tenant_bill_collection
             (id, bill_id, tenant_id, note, promised_amount_cents, promised_date, created_by)
         VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6)
         RETURNING id, bill_id, note, promised_amount_cents, promised_date, created_at",
    )
    .bind(id)
    .bind(tenant_id)
    .bind(req.note.trim())
    .bind(req.promised_amount_cents)
    .bind(req.promised_date)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.tenant.bill.collection",
        "sys_tenant_bill",
        Some(&id.to_string()),
        Some(serde_json::json!({
            "note": req.note,
            "promisedAmountCents": req.promised_amount_cents,
            "promisedDate": req.promised_date
        })),
    )
    .await;
    Ok(ApiResponse::new(vo))
}

/// 催收提醒扫描结果。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CollectionReminderResult {
    /// 本次产生提醒的记录数。
    pub scanned: u64,
}

/// 扫描承诺日期临近的催收记录（7 天内不重复）。
pub async fn scan_collection_reminders(pool: &sqlx::PgPool, days: i64) -> anyhow::Result<u64> {
    // 催收表已 FORCE RLS：逐租户设置上下文，否则扫描永远为 0。
    let tenants: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM sys.sys_tenant WHERE deleted_at IS NULL")
            .fetch_all(pool)
            .await?;
    let mut scanned = 0;
    for tenant in tenants {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;

        let rows: Vec<(Uuid, Uuid, Option<i64>)> = sqlx::query_as(
            "SELECT id, tenant_id, promised_amount_cents
             FROM sys.sys_tenant_bill_collection
             WHERE tenant_id = $2
               AND promised_date IS NOT NULL
               AND promised_date <= current_date + make_interval(days => $1::int)
               AND (reminded_at IS NULL OR reminded_at < now() - interval '7 days')",
        )
        .bind(days as i32)
        .bind(tenant)
        .fetch_all(&mut *tx)
        .await?;

        for (id, tenant_id, promised) in rows {
            crate::audit::record_alert(
                pool,
                tenant_id,
                "system.tenant.bill.collection_due",
                promised.unwrap_or(0),
                days.min(i32::MAX as i64) as i32,
            )
            .await?;
            sqlx::query(
                "UPDATE sys.sys_tenant_bill_collection
                 SET reminded_at = now()
                 WHERE id = $1 AND tenant_id = $2",
            )
            .bind(id)
            .bind(tenant)
            .execute(&mut *tx)
            .await?;
            scanned += 1;
        }
        tx.commit().await?;
    }
    Ok(scanned)
}

/// 手动触发催收承诺提醒扫描。
#[utoipa::path(
    post,
    path = "/api/system/tenants/bills/collections/reminders/scan",
    tag = "system",
    params(("days" = Option<i64>, Query, description = "提前天数，默认 3")),
    responses((status = 200, description = "扫描结果", body = ApiResponse<CollectionReminderResult>))
)]
async fn scan_collection_reminders_handler(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<TenantUsageQuery>,
) -> Result<ApiResponse<CollectionReminderResult>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_EDIT)?;
    let days = q.days.unwrap_or(3).clamp(0, 365);
    let scanned = scan_collection_reminders(&state.db, days)
        .await
        .map_err(|e| AppError::Internal(e.context("催收提醒扫描失败")))?;
    Ok(ApiResponse::new(CollectionReminderResult { scanned }))
}
