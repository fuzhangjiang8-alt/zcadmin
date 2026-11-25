//! 租户详情汇总接口。
//!
//! 边界：租户生命周期在 `tenant_admin`；本模块只读聚合各域计数。
//! 关键约束：聚合查询需先设置租户上下文（RLS）。

use axum::extract::{Path, State};
use axum::routing::get;
use axum::Router;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::{require_perm, require_platform};

/// 租户汇总路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/tenants/{id}/summary", get(tenant_summary))
}

/// 租户审计摘要。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantAuditBrief {
    /// 动作。
    pub action: String,
    /// 时间。
    pub created_at: DateTime<Utc>,
}

/// 租户详情汇总。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantSummaryVo {
    /// 租户 id。
    pub id: Uuid,
    /// 编码。
    pub code: String,
    /// 名称。
    pub name: String,
    /// 状态。
    pub status: String,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 用户上限。
    pub max_users: i32,
    /// 存储上限。
    pub max_storage_bytes: i64,
    /// 用户数。
    pub user_count: i64,
    /// 文件数。
    pub file_count: i64,
    /// 存储使用。
    pub storage_used: i64,
    /// 审计条数。
    pub audit_count: i64,
    /// 活跃会话数。
    pub session_count: i64,
    /// CRM 客户数。
    pub customer_count: i64,
    /// CRM 商机数。
    pub opportunity_count: i64,
    /// CRM 联系人数。
    pub contact_count: i64,
    /// CRM 合同数。
    pub contract_count: i64,
    /// CRM 回款数。
    pub payment_count: i64,
    /// 最近审计。
    pub recent_audits: Vec<TenantAuditBrief>,
}

/// 租户详情汇总。
#[utoipa::path(
    get,
    path = "/api/system/tenants/{id}/summary",
    tag = "system",
    params(("id" = Uuid, Path, description = "租户 id")),
    responses((status = 200, description = "租户汇总", body = ApiResponse<TenantSummaryVo>))
)]
async fn tenant_summary(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<TenantSummaryVo>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_LIST)?;
    let mut tx = state.db.begin().await?;
    let info: Option<(String, String, String, DateTime<Utc>, i32, i64)> = sqlx::query_as(
        "SELECT code, name, status, created_at, max_users, max_storage_bytes
         FROM sys.sys_tenant WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((code, name, status, created_at, max_users, max_storage_bytes)) = info else {
        return Err(AppError::NotFound);
    };
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;
    let user_count: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM sys.sys_user WHERE tenant_id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let file_count: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM sys.sys_file WHERE tenant_id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let storage_used: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(size_bytes), 0)::bigint FROM sys.sys_file
         WHERE tenant_id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let audit_count: i64 =
        sqlx::query_scalar("SELECT count(*)::bigint FROM sys.sys_audit_log WHERE tenant_id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    let session_count: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM sys.sys_session
         WHERE tenant_id = $1 AND expires_at > now()",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    let has_crm: bool = sqlx::query_scalar("SELECT to_regclass('crm.customer') IS NOT NULL")
        .fetch_one(&mut *tx)
        .await?;
    let (customer_count, opportunity_count, contact_count, contract_count, payment_count) =
        if has_crm {
            let customers: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM crm.customer WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
            let opportunities: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM crm.opportunity WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
            let contacts: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM crm.contact WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
            let contracts: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM crm.contract WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
            let payments: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM crm.payment WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
            (customers, opportunities, contacts, contracts, payments)
        } else {
            (0, 0, 0, 0, 0)
        };
    let recent_audits: Vec<TenantAuditBrief> = sqlx::query_as(
        "SELECT action, created_at FROM sys.sys_audit_log
         WHERE tenant_id = $1 ORDER BY created_at DESC LIMIT 10",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(TenantSummaryVo {
        id,
        code,
        name,
        status,
        created_at,
        max_users,
        max_storage_bytes,
        user_count,
        file_count,
        storage_used,
        audit_count,
        session_count,
        customer_count,
        opportunity_count,
        contact_count,
        contract_count,
        payment_count,
        recent_audits,
    }))
}
