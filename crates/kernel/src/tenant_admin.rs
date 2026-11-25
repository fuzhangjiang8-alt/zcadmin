//! 租户管理、配额、账单、计费与催收。
//!
//! # 职责
//!
//! 平台级（`system:tenant:*`）能力：租户生命周期、用户/存储配额、
//! 月度账单、收款/退款/催收、用量快照与逾期扫描。
//!
//! # 边界
//!
//! - 只处理平台级数据；租户内业务（用户/角色/菜单）在 `system` 模块；
//! - 账单表已 FORCE RLS：平台超管通过 `app.platform='on'` 跨租户读取，
//!   后台任务逐租户 `set_config('app.tenant_id')`；
//! - 所有写接口必须有 `system:tenant:*` 权限点。
//!
//! # 关键约束
//!
//! 从 `system.rs` 拆出（原文件 7.9k 行）。路由由 [`router`] 提供，
//! 由 `system::routes()` 合并；OpenAPI 片段由 [`TenantApi`] 提供。

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::{require_perm, require_platform};
use crate::tenant::TenantTx;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// 租户与计费路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .merge(crate::tenant_bill_export::router())
        .merge(crate::tenant_lifecycle::router())
        .merge(crate::tenant_create::router())
        .merge(crate::tenant_purge_api::router())
        .merge(crate::tenant_summary::router())
        .merge(crate::tenant_bills::router())
        .merge(crate::tenant_bill_backfill::router())
        .merge(crate::tenant_billing::router())
        .merge(crate::tenant_usage_api::router())
        .merge(crate::tenant_overdue::router())
        .route("/tenants", get(list_tenants))
        .merge(crate::tenant_bill_collections::router())
        .merge(crate::tenant_bill_payments::router())
}

/// 租户列表查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantQuery {
    /// 是否包含已删除租户。
    pub include_deleted: Option<bool>,
}

/// 租户视图。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantVo {
    /// 租户 id。
    pub id: Uuid,
    /// 租户编码。
    pub code: String,
    /// 租户名称。
    pub name: String,
    /// 状态：active / suspended。
    pub status: String,
    /// 用户数。
    pub user_count: i64,
    /// 用户数上限（0 不限制）。
    pub max_users: i32,
    /// 存储上限字节（0 不限制）。
    pub max_storage_bytes: i64,
    /// 已使用存储字节。
    pub storage_used: i64,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 租户列表。
#[utoipa::path(
    get,
    path = "/api/system/tenants",
    tag = "system",
    responses((status = 200, description = "租户列表", body = ApiResponse<Vec<TenantVo>>))
)]
async fn list_tenants(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<TenantQuery>,
) -> Result<ApiResponse<Vec<TenantVo>>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_LIST)?;
    let include_deleted = q.include_deleted.unwrap_or(false);
    let rows: Vec<TenantVo> = sqlx::query_as(
        r#"
        SELECT t.id, t.code, t.name, t.status, t.created_at, t.max_users, t.max_storage_bytes,
               (SELECT count(*)::bigint FROM sys.sys_user u
                WHERE u.tenant_id = t.id AND u.deleted_at IS NULL) AS user_count,
               (SELECT COALESCE(sum(f.size_bytes), 0)::bigint FROM sys.sys_file f
                WHERE f.tenant_id = t.id AND f.deleted_at IS NULL) AS storage_used
        FROM sys.sys_tenant t
        WHERE ($1::bool OR t.deleted_at IS NULL)
        ORDER BY t.created_at, t.id
        "#,
    )
    .bind(include_deleted)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 为所有活跃租户补齐默认告警规则（幂等）。
pub async fn ensure_default_alert_rules(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    let tenants: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM sys.sys_tenant WHERE deleted_at IS NULL")
            .fetch_all(pool)
            .await?;
    for tenant in tenants {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await?;
        for (pattern, window, threshold) in [
            ("*", 5, 50),
            ("auth.login.failed", 5, 20),
            ("system.import", 5, 10),
        ] {
            sqlx::query(
                "INSERT INTO sys.sys_audit_alert_rule
                     (id, tenant_id, action_pattern, window_minutes, threshold)
                 VALUES (gen_random_uuid(), $1, $2, $3, $4)
                 ON CONFLICT DO NOTHING",
            )
            .bind(tenant)
            .bind(pattern)
            .bind(window)
            .bind(threshold)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
    }
    Ok(())
}

/// 租户/计费接口的 OpenAPI 片段。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list_tenants,
        crate::tenant_create::create_tenant,
        crate::tenant_lifecycle::update_tenant_status,
        crate::tenant_lifecycle::update_tenant_quota,
        crate::tenant_lifecycle::delete_tenant,
        crate::tenant_lifecycle::restore_tenant,
        crate::tenant_purge_api::purge_tenants,
        crate::tenant_summary::tenant_summary,
        crate::tenant_usage_api::capture_tenant_usage,
        crate::tenant_usage_api::tenant_usage,
        crate::tenant_billing::update_tenant_billing,
        crate::tenant_billing::tenant_bill,
        crate::tenant_billing::export_tenant_bills,
        crate::tenant_bill_export::export_tenant_bills_xlsx,
        crate::tenant_bills::generate_tenant_bills,
        crate::tenant_bills::list_tenant_bills,
        crate::tenant_bills::bill_reconciliation,
        crate::tenant_bill_backfill::backfill_tenant_bills,
        crate::tenant_overdue::scan_overdue_bills_handler,
        crate::tenant_bill_collections::scan_collection_reminders_handler,
        crate::tenant_bills::update_bill_status,
        crate::tenant_bill_payments::list_bill_payments,
        crate::tenant_bill_payments::create_bill_payment,
        crate::tenant_bill_payments::create_bill_refund,
        crate::tenant_bill_collections::list_bill_collections,
        crate::tenant_bill_collections::create_bill_collection
    ),
    components(schemas(
        TenantVo,
        TenantQuery,
        crate::tenant_create::CreateTenantRequest,
        crate::tenant_lifecycle::TenantStatusRequest,
        crate::tenant_lifecycle::TenantQuotaRequest,
        crate::tenant_purge_api::TenantPurgeQuery,
        crate::tenant_purge_api::TenantPurgeResult,
        crate::tenant_purge_api::TenantPurgeDetail,
        crate::tenant_summary::TenantSummaryVo,
        crate::tenant_summary::TenantAuditBrief,
        crate::tenant_usage_api::TenantUsageQuery,
        crate::tenant_bills::TenantBillQuery,
        crate::tenant_billing::TenantBillVo,
        crate::tenant_billing::TenantBillingRequest,
        crate::tenant_bills::TenantBillRow,
        crate::tenant_bills::BillGenerateResult,
        crate::tenant_bills::BillStatusRequest,
        crate::tenant_bill_payments::BillPaymentVo,
        crate::tenant_bill_payments::BillPaymentRequest,
        crate::tenant_bill_payments::BillRefundRequest,
        crate::tenant_bill_collections::BillCollectionVo,
        crate::tenant_bill_collections::BillCollectionRequest,
        crate::tenant_bills::ReconciliationRow,
        crate::tenant_bill_backfill::BillBackfillQuery,
        crate::tenant_bill_backfill::BillBackfillResult,
        crate::tenant_overdue::OverdueScanResult,
        crate::tenant_bill_collections::CollectionReminderResult,
        crate::tenant_usage_api::UsageCaptureResult,
        crate::tenant_usage::UsagePoint
    ))
)]
pub struct TenantApi;
