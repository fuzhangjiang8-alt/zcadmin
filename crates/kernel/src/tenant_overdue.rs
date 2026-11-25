//! 逾期账单扫描与告警。
//!
//! 边界：账单主体在 `tenant_admin`；本模块只做逾期扫描与手动触发。
//! 关键约束：`sys_tenant_bill` 已 FORCE RLS，必须逐租户设置上下文后扫描。

use axum::extract::{Query, State};
use axum::routing::post;
use axum::Router;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::{require_perm, require_platform};
use crate::tenant_usage_api::TenantUsageQuery;

/// 逾期扫描路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route(
        "/tenants/bills/overdue/scan",
        post(scan_overdue_bills_handler),
    )
}

/// 逾期扫描结果。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OverdueScanResult {
    /// 本次产生告警的账单数。
    pub scanned: u64,
}

type OverdueRow = (Uuid, Uuid, i64, i64, i32, i32, Option<DateTime<Utc>>);

/// 扫描逾期未收账单并写告警（7 天内不重复）。
pub async fn scan_overdue_bills_internal(
    pool: &sqlx::PgPool,
    overdue_days: i64,
) -> anyhow::Result<u64> {
    // sys_tenant_bill 已 FORCE RLS：必须逐租户设置上下文后再扫描，
    // 直接用 pool 裸查会返回 0 行（逾期提醒静默失效）。
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

        let rows: Vec<OverdueRow> = sqlx::query_as(
            r#"
            SELECT b.id, b.tenant_id, b.total_cents,
                   COALESCE((SELECT sum(CASE WHEN p.kind = 'refund' THEN -p.amount_cents ELSE p.amount_cents END)
                             FROM sys.sys_tenant_bill_payment p WHERE p.bill_id = b.id), 0)::bigint,
                   GREATEST(0, current_date - (b.bill_month + interval '1 month')::date) AS overdue_days,
                   b.overdue_tier, b.overdue_notified_at
            FROM sys.sys_tenant_bill b
            WHERE b.tenant_id = $2
              AND b.status = 'issued'
              AND b.bill_month + interval '1 month' + make_interval(days => $1::int) < current_date
            "#,
        )
        .bind(overdue_days as i32)
        .bind(tenant)
        .fetch_all(&mut *tx)
        .await?;

        for (bill_id, tenant_id, total, paid, days_late, old_tier, notified) in rows {
            let tier = if days_late >= 90 {
                90
            } else if days_late >= 60 {
                60
            } else {
                30
            };
            let due_again = notified
                .map(|t| (Utc::now() - t).num_days() >= 7)
                .unwrap_or(true);
            if !due_again && tier <= old_tier {
                continue;
            }
            let outstanding = (total - paid).max(0);
            crate::audit::record_alert(
                pool,
                tenant_id,
                &format!("system.tenant.bill.overdue.{tier}"),
                outstanding,
                days_late,
            )
            .await?;
            sqlx::query(
                "UPDATE sys.sys_tenant_bill
                 SET overdue_notified_at = now(), overdue_tier = $1
                 WHERE id = $2 AND tenant_id = $3",
            )
            .bind(tier)
            .bind(bill_id)
            .bind(tenant)
            .execute(&mut *tx)
            .await?;
            scanned += 1;
        }
        tx.commit().await?;
    }
    Ok(scanned)
}

/// 手动触发逾期账单扫描。
#[utoipa::path(
    post,
    path = "/api/system/tenants/bills/overdue/scan",
    tag = "system",
    params(("days" = Option<i64>, Query, description = "逾期宽限天数，默认 30")),
    responses((status = 200, description = "扫描结果", body = ApiResponse<OverdueScanResult>))
)]
async fn scan_overdue_bills_handler(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<TenantUsageQuery>,
) -> Result<ApiResponse<OverdueScanResult>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_EDIT)?;
    let days = q.days.unwrap_or(30).clamp(0, 3650);
    let scanned = scan_overdue_bills_internal(&state.db, days)
        .await
        .map_err(|e| AppError::Internal(e.context("逾期扫描失败")))?;
    Ok(ApiResponse::new(OverdueScanResult { scanned }))
}
