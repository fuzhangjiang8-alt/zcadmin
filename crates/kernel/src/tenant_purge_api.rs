//! 租户物理清理接口（超过保留期的已删除租户）。
//!
//! 边界：租户软删除/恢复在 `tenant_admin`；清理执行在 `tenant_purge` 领域模块。
//! 关键约束：默认 30 天保留期；dry-run 只统计不删除。

use axum::extract::{Query, State};
use axum::routing::post;
use axum::Router;
use serde::{Deserialize, Serialize};

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::{require_perm, require_platform};

/// 清理路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/tenants/purge", post(purge_tenants))
}

/// 租户物理清理参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantPurgeQuery {
    /// 保留天数（默认 30）。
    pub retention_days: Option<i64>,
    /// 仅统计不删除。
    pub dry_run: Option<bool>,
}

/// 单个租户清理明细。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantPurgeDetail {
    /// 租户编码。
    pub code: String,
    /// 删除的用户数。
    pub users: i64,
    /// 删除的文件数。
    pub files: i64,
}

/// 租户物理清理结果。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantPurgeResult {
    /// 命中的租户编码。
    pub tenants: Vec<String>,
    /// 实际删除数。
    pub purged: u64,
    /// 是否 dry-run。
    pub dry_run: bool,
    /// 清理明细。
    pub details: Vec<TenantPurgeDetail>,
}

/// 物理清理超过保留期的已删除租户。
#[utoipa::path(
    post,
    path = "/api/system/tenants/purge",
    tag = "system",
    params(
        ("retentionDays" = Option<i64>, Query, description = "保留天数，默认 30"),
        ("dryRun" = Option<bool>, Query, description = "仅统计")
    ),
    responses((status = 200, description = "清理结果", body = ApiResponse<TenantPurgeResult>))
)]
async fn purge_tenants(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<TenantPurgeQuery>,
) -> Result<ApiResponse<TenantPurgeResult>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_DELETE)?;
    let retention_days = q.retention_days.unwrap_or(30).clamp(0, 3650);
    let dry_run = q.dry_run.unwrap_or(false);
    let outcome = crate::tenant_purge::purge_deleted_tenants(&state.db, retention_days, dry_run)
        .await
        .map_err(|e| AppError::Internal(e.context("清理租户失败")))?;
    let details: Vec<TenantPurgeDetail> = outcome
        .details
        .iter()
        .map(|d| TenantPurgeDetail {
            code: d.code.clone(),
            users: d.users,
            files: d.files,
        })
        .collect();
    crate::audit::record_user(
        &state.db,
        &user,
        if dry_run {
            "system.tenant.purge.preview"
        } else {
            "system.tenant.purge"
        },
        "sys_tenant",
        None,
        Some(serde_json::json!({
            "retentionDays": retention_days,
            "tenants": outcome.tenants,
            "purged": outcome.purged,
            "details": details
        })),
    )
    .await;
    Ok(ApiResponse::new(TenantPurgeResult {
        tenants: outcome.tenants,
        purged: outcome.purged,
        dry_run,
        details,
    }))
}
