//! 审计配置快照工具：导出、对比与改名。
//!
//! 边界：快照列表/删除/清理在 `audit_config_snapshots`。
//! 关键约束：导出不包含 secret；对比按 sections 过滤。

use axum::extract::{Path, Query, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::audit_config_bundle::{compute_config_diff, AuditConfigBundle, AuditConfigDiffVo};
use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;

/// 配置快照工具路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/audit-config/snapshots/compare",
            get(compare_config_snapshots),
        )
        .route(
            "/audit-config/snapshots/{id}/export",
            get(export_config_snapshot),
        )
        .route("/audit-config/snapshots/{id}", put(update_config_snapshot))
}

/// 导出指定快照的配置包。
#[utoipa::path(
    get,
    path = "/api/system/audit-config/snapshots/{id}/export",
    tag = "system",
    params(("id" = Uuid, Path, description = "快照 id")),
    responses((status = 200, description = "配置包", body = ApiResponse<AuditConfigBundle>))
)]
async fn export_config_snapshot(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<AuditConfigBundle>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let row: Option<(sqlx::types::Json<AuditConfigBundle>,)> = sqlx::query_as(
        "SELECT bundle FROM sys.sys_audit_config_snapshot WHERE id = $1 AND tenant_id = $2",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    let (bundle,) = row.ok_or(AppError::NotFound)?;
    Ok(ApiResponse::new(bundle.0))
}

/// 快照对比参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotCompareQuery {
    /// 左侧快照 id。
    pub left: Uuid,
    /// 右侧快照 id。
    pub right: Uuid,
}

/// 对比两个快照（左 → 右）。
#[utoipa::path(
    get,
    path = "/api/system/audit-config/snapshots/compare",
    tag = "system",
    params(
        ("left" = Uuid, Query, description = "左侧快照"),
        ("right" = Uuid, Query, description = "右侧快照")
    ),
    responses((status = 200, description = "差异", body = ApiResponse<AuditConfigDiffVo>))
)]
async fn compare_config_snapshots(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<SnapshotCompareQuery>,
) -> Result<ApiResponse<AuditConfigDiffVo>, AppError> {
    require_perm(&user, crate::perms::AUDIT_LIST)?;
    let left: Option<(sqlx::types::Json<AuditConfigBundle>,)> = sqlx::query_as(
        "SELECT bundle FROM sys.sys_audit_config_snapshot WHERE id = $1 AND tenant_id = $2",
    )
    .bind(q.left)
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    let right: Option<(sqlx::types::Json<AuditConfigBundle>,)> = sqlx::query_as(
        "SELECT bundle FROM sys.sys_audit_config_snapshot WHERE id = $1 AND tenant_id = $2",
    )
    .bind(q.right)
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    let (left,) = left.ok_or(AppError::NotFound)?;
    let (right,) = right.ok_or(AppError::NotFound)?;
    Ok(ApiResponse::new(compute_config_diff(&left.0, &right.0)))
}

/// 快照命名请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotUpdateRequest {
    /// 名称（最长 100）。
    pub name: Option<String>,
    /// 备注（最长 500）。
    pub note: Option<String>,
}

/// 更新快照名称/备注。
#[utoipa::path(
    put,
    path = "/api/system/audit-config/snapshots/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "快照 id")),
    request_body = SnapshotUpdateRequest,
    responses((status = 200, description = "已更新"))
)]
async fn update_config_snapshot(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<SnapshotUpdateRequest>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::AUDIT_ALERT)?;
    if req.name.as_ref().is_some_and(|n| n.len() > 100) {
        return Err(AppError::Validation("快照名称过长".into()));
    }
    if req.note.as_ref().is_some_and(|n| n.len() > 500) {
        return Err(AppError::Validation("快照备注过长".into()));
    }
    let result = sqlx::query(
        "UPDATE sys.sys_audit_config_snapshot SET name = $3, note = $4
         WHERE id = $1 AND tenant_id = $2",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(&req.name)
    .bind(&req.note)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;
    Ok(ApiResponse::new(()))
}
