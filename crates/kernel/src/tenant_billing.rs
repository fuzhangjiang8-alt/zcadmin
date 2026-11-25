//! 租户计费配置、账单预估与账单导出（CSV/JSON）。
//!
//! 边界：账单生成/列表/状态在 `tenant_admin`；Excel 导出在 `tenant_bill_export`。
//! 关键约束：计费配置按租户保存；账单预估按当月用量计算。

use axum::extract::{Path, Query, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::{require_perm, require_platform};
use crate::tenant::TenantTx;
use crate::tenant_bills::TenantBillQuery;

/// 计费/账单路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/tenants/{id}/billing", put(update_tenant_billing))
        .route("/tenants/{id}/bill", get(tenant_bill))
        .route("/tenants/bills/export", get(export_tenant_bills))
}

/// 租户计费配置请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantBillingRequest {
    /// 每用户每月单价（分）。
    pub price_per_user_month_cents: i32,
    /// 每 GB 每月单价（分）。
    pub price_per_gb_month_cents: i32,
    /// 币种。
    pub currency: String,
}

/// 租户账单预估。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantBillVo {
    /// 月份 YYYY-MM。
    pub month: String,
    /// 币种。
    pub currency: String,
    /// 平均用户数。
    pub avg_users: f64,
    /// 平均存储字节。
    pub avg_storage_bytes: f64,
    /// 用户费用（分）。
    pub user_amount_cents: i64,
    /// 存储费用（分）。
    pub storage_amount_cents: i64,
    /// 合计（分）。
    pub total_cents: i64,
}

/// 更新租户计费单价。
#[utoipa::path(
    put,
    path = "/api/system/tenants/{id}/billing",
    tag = "system",
    params(("id" = Uuid, Path, description = "租户 id")),
    request_body = TenantBillingRequest,
    responses((status = 200, description = "已更新"))
)]
async fn update_tenant_billing(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<TenantBillingRequest>,
) -> Result<ApiResponse<()>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_EDIT)?;
    if req.price_per_user_month_cents < 0 || req.price_per_gb_month_cents < 0 {
        return Err(AppError::Validation("计费单价不能为负".into()));
    }
    if req.currency.len() != 3 {
        return Err(AppError::Validation("币种必须为 3 位代码".into()));
    }
    let affected = sqlx::query(
        "UPDATE sys.sys_tenant
         SET price_per_user_month_cents = $1,
             price_per_gb_month_cents = $2,
             currency = $3,
             updated_at = now()
         WHERE id = $4 AND deleted_at IS NULL",
    )
    .bind(req.price_per_user_month_cents)
    .bind(req.price_per_gb_month_cents)
    .bind(req.currency.to_uppercase())
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.tenant.billing",
        "sys_tenant",
        Some(&id.to_string()),
        Some(serde_json::json!({
            "pricePerUserMonthCents": req.price_per_user_month_cents,
            "pricePerGbMonthCents": req.price_per_gb_month_cents,
            "currency": req.currency
        })),
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 查询租户月度账单预估。
#[utoipa::path(
    get,
    path = "/api/system/tenants/{id}/bill",
    tag = "system",
    params(
        ("id" = Uuid, Path, description = "租户 id"),
        ("month" = Option<String>, Query, description = "YYYY-MM，默认当月")
    ),
    responses((status = 200, description = "账单预估", body = ApiResponse<TenantBillVo>))
)]
async fn tenant_bill(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
    Query(q): Query<TenantBillQuery>,
) -> Result<ApiResponse<TenantBillVo>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_LIST)?;
    let month = q
        .month
        .filter(|m| m.len() == 7)
        .unwrap_or_else(|| Utc::now().format("%Y-%m").to_string());
    let (currency, price_user, price_gb): (String, i32, i32) = sqlx::query_as(
        "SELECT currency, price_per_user_month_cents, price_per_gb_month_cents
         FROM sys.sys_tenant WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)?;
    let (avg_users, avg_storage) = query_monthly_usage(&state.db, id, &month).await?;
    let user_amount = (avg_users * f64::from(price_user)).ceil() as i64;
    let storage_gb = avg_storage / 1024.0 / 1024.0 / 1024.0;
    let storage_amount = (storage_gb * f64::from(price_gb)).ceil() as i64;
    Ok(ApiResponse::new(TenantBillVo {
        month,
        currency,
        avg_users,
        avg_storage_bytes: avg_storage,
        user_amount_cents: user_amount,
        storage_amount_cents: storage_amount,
        total_cents: user_amount + storage_amount,
    }))
}

/// 查询租户指定月份的平均用户数与平均存储字节（无日快照时回退查询当前实时数据）。
async fn query_monthly_usage(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
    month: &str,
) -> Result<(f64, f64), AppError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await?;
    let (avg_users, avg_storage, snapshots): (f64, f64, i64) = sqlx::query_as(
        "SELECT COALESCE(AVG(user_count), 0)::float8,
                COALESCE(AVG(storage_used), 0)::float8,
                count(*)::bigint
         FROM sys.sys_tenant_usage_snapshot
         WHERE tenant_id = $1
           AND snapshot_date >= to_date($2 || '-01', 'YYYY-MM-DD')
           AND snapshot_date < to_date($2 || '-01', 'YYYY-MM-DD') + interval '1 month'",
    )
    .bind(tenant_id)
    .bind(month)
    .fetch_one(&mut *tx)
    .await?;
    let result = if snapshots == 0 {
        let users: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM sys.sys_user WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await?;
        let storage: i64 = sqlx::query_scalar(
            "SELECT COALESCE(sum(size_bytes), 0)::bigint FROM sys.sys_file WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(tenant_id)
        .fetch_one(&mut *tx)
        .await?;
        (users as f64, storage as f64)
    } else {
        (avg_users, avg_storage)
    };
    tx.commit().await?;
    Ok(result)
}

/// 导出所有租户月度账单（CSV）。
#[utoipa::path(
    get,
    path = "/api/system/tenants/bills/export",
    tag = "system",
    params(("month" = Option<String>, Query, description = "YYYY-MM，默认当月")),
    responses((status = 200, description = "CSV 文件"))
)]
async fn export_tenant_bills(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<TenantBillQuery>,
) -> Result<axum::response::Response, AppError> {
    use axum::response::IntoResponse;
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_LIST)?;
    let month = q
        .month
        .filter(|m| m.len() == 7)
        .unwrap_or_else(|| Utc::now().format("%Y-%m").to_string());
    let tenants: Vec<(Uuid, String, String, String, i32, i32)> = sqlx::query_as(
        "SELECT id, code, name, currency, price_per_user_month_cents, price_per_gb_month_cents
         FROM sys.sys_tenant WHERE deleted_at IS NULL ORDER BY code",
    )
    .fetch_all(&state.db)
    .await?;
    let mut csv = String::from(
        "tenant_code,tenant_name,month,currency,avg_users,avg_storage_gb,user_amount_cents,storage_amount_cents,total_cents\n",
    );
    for (id, code, name, currency, price_user, price_gb) in tenants {
        let (avg_users, avg_storage) = query_monthly_usage(&state.db, id, &month).await?;
        let storage_gb = avg_storage / 1024.0 / 1024.0 / 1024.0;
        let user_amount = (avg_users * f64::from(price_user)).ceil() as i64;
        let storage_amount = (storage_gb * f64::from(price_gb)).ceil() as i64;
        csv.push_str(&format!(
            "{},{},{},{},{:.2},{:.4},{},{},{}\n",
            escape_csv(&code),
            escape_csv(&name),
            month,
            currency,
            avg_users,
            storage_gb,
            user_amount,
            storage_amount,
            user_amount + storage_amount
        ));
    }
    let filename = format!("tenant-bills-{month}.csv");
    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "text/csv; charset=utf-8".to_string(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        csv,
    )
        .into_response())
}

fn escape_csv(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}
