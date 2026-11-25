//! 租户状态、配额、删除与恢复接口。
//!
//! 边界：新建在 `tenant_create`；列表在 `tenant_admin`；物理清理在 `tenant_purge_api`。
//! 关键约束：不能删除/停用当前登录租户；状态变更与配额调整均写审计。

use axum::extract::{Path, State};
use axum::routing::{delete, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::{require_perm, require_platform};
use crate::tenant::TenantTx;

/// 租户生命周期路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/tenants/{id}/status", put(update_tenant_status))
        .route("/tenants/{id}/quota", put(update_tenant_quota))
        .route("/tenants/{id}", delete(delete_tenant))
        .route("/tenants/{id}/restore", post(restore_tenant))
}

/// 租户状态请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantStatusRequest {
    /// active / suspended。
    pub status: String,
}

/// 租户停用/删除后吊销其全部会话与访问令牌。
///
/// 访问令牌（PAT）是独立凭据，不随会话撤销失效；只清会话等于
/// 「租户被停用了，但既有令牌还能继续访问自己的数据」。
async fn revoke_tenant_credentials(state: &AppState, tenant_id: Uuid) {
    let _ = state.sessions.revoke_tenant_sessions(tenant_id).await;
    match crate::auth_tokens::revoke_tenant_tokens(&state.db, tenant_id).await {
        Ok(n) if n > 0 => {
            tracing::info!(tenant = %tenant_id, revoked_tokens = n, "租户停用，已吊销访问令牌");
        }
        Ok(_) => {}
        Err(e) => tracing::error!(error = %e, tenant = %tenant_id, "吊销租户访问令牌失败"),
    }
}

/// 启用/停用租户。
#[utoipa::path(
    put,
    path = "/api/system/tenants/{id}/status",
    tag = "system",
    request_body = TenantStatusRequest,
    params(("id" = Uuid, Path, description = "租户 id")),
    responses((status = 200, description = "已更新"))
)]
async fn update_tenant_status(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<TenantStatusRequest>,
) -> Result<ApiResponse<()>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_EDIT)?;
    if !matches!(req.status.as_str(), "active" | "suspended") {
        return Err(AppError::Validation("非法租户状态".into()));
    }
    if id == user.tenant_id && req.status == "suspended" {
        return Err(AppError::Validation("不能停用当前租户".into()));
    }
    let affected = sqlx::query(
        "UPDATE sys.sys_tenant SET status = $1, updated_at = now()
         WHERE id = $2 AND deleted_at IS NULL",
    )
    .bind(&req.status)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;
    if req.status == "suspended" {
        revoke_tenant_credentials(&state, id).await;
    }
    crate::audit::record_user(
        &state.db,
        &user,
        "system.tenant.status",
        "sys_tenant",
        Some(&id.to_string()),
        Some(serde_json::json!({ "status": req.status })),
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 租户配额请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TenantQuotaRequest {
    /// 用户数上限（0 不限制）。
    pub max_users: i32,
    /// 存储上限字节（0 不限制）。
    pub max_storage_bytes: Option<i64>,
}

/// 更新租户用户配额。
#[utoipa::path(
    put,
    path = "/api/system/tenants/{id}/quota",
    tag = "system",
    request_body = TenantQuotaRequest,
    params(("id" = Uuid, Path, description = "租户 id")),
    responses((status = 200, description = "已更新"))
)]
async fn update_tenant_quota(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<TenantQuotaRequest>,
) -> Result<ApiResponse<()>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_EDIT)?;
    if !(0..=100_000).contains(&req.max_users) {
        return Err(AppError::Validation("用户数上限必须在 0-100000".into()));
    }
    if let Some(v) = req.max_storage_bytes {
        if !(0..=1_000_000_000_000_000).contains(&v) {
            return Err(AppError::Validation("存储上限超出范围".into()));
        }
    }
    let current: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM sys.sys_user WHERE tenant_id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if req.max_users > 0 && current > i64::from(req.max_users) {
        return Err(AppError::Validation("当前用户数已超过新上限".into()));
    }
    let affected = sqlx::query(
        "UPDATE sys.sys_tenant
         SET max_users = $1,
             max_storage_bytes = COALESCE($2, max_storage_bytes),
             updated_at = now()
         WHERE id = $3 AND deleted_at IS NULL",
    )
    .bind(req.max_users)
    .bind(req.max_storage_bytes)
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
        "system.tenant.quota",
        "sys_tenant",
        Some(&id.to_string()),
        Some(serde_json::json!({
            "maxUsers": req.max_users,
            "maxStorageBytes": req.max_storage_bytes
        })),
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 软删除租户。
#[utoipa::path(
    delete,
    path = "/api/system/tenants/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "租户 id")),
    responses((status = 200, description = "已删除"))
)]
async fn delete_tenant(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_DELETE)?;
    if id == user.tenant_id {
        return Err(AppError::Validation("不能删除当前租户".into()));
    }
    let affected = sqlx::query(
        "UPDATE sys.sys_tenant SET status = 'deleted', deleted_at = now(), updated_at = now() WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if affected == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await?;
    revoke_tenant_credentials(&state, id).await;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.tenant.delete",
        "sys_tenant",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 恢复已删除租户。
#[utoipa::path(
    post,
    path = "/api/system/tenants/{id}/restore",
    tag = "system",
    params(("id" = Uuid, Path, description = "租户 id")),
    responses((status = 200, description = "已恢复"))
)]
async fn restore_tenant(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_EDIT)?;
    let affected = sqlx::query(
        "UPDATE sys.sys_tenant
         SET status = 'active', deleted_at = NULL, updated_at = now()
         WHERE id = $1 AND deleted_at IS NOT NULL",
    )
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
        "system.tenant.restore",
        "sys_tenant",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}
