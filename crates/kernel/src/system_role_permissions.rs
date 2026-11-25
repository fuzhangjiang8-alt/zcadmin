//! 角色-权限点授权接口（查询与全量覆盖）。
//!
//! 边界：角色主体在 `system_roles`；用户绑定在 `system_user_roles`。
//! 关键约束：目标角色必须属于当前租户；全量覆盖在单事务内完成。

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system_identity::{ensure_role_in_tenant, require_perm};
use crate::tenant::TenantTx;

/// 角色授权与权限点查询路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/permissions", get(list_permissions))
        .route(
            "/roles/{id}/permissions",
            get(get_role_permissions).put(set_role_permissions),
        )
}

/// 权限点视图对象。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PermissionVo {
    /// 权限点 id。
    pub id: Uuid,
    /// 所属插件。
    pub plugin: String,
    /// 权限点编码，例如 crm:customer:add。
    pub code: String,
    /// 展示名称。
    pub name: String,
    /// 是否已下线。
    pub deprecated: bool,
}

/// 权限点列表（平台级权限点仅对平台超管可见）。
#[utoipa::path(
    get,
    path = "/api/system/permissions",
    tag = "system",
    responses((status = 200, description = "权限点列表（按调用者身份过滤）", body = ApiResponse<Vec<PermissionVo>>))
)]
pub(crate) async fn list_permissions(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<Vec<PermissionVo>>, AppError> {
    require_perm(&user, crate::perms::ROLE_LIST)?;
    let rows = sqlx::query_as::<_, PermissionVo>(
        "SELECT id, plugin, code, name, deprecated FROM sys.sys_permission
         WHERE ($1::bool OR scope = 'tenant') ORDER BY plugin, code",
    )
    .bind(user.is_super_admin())
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 批量 id 请求体（角色权限、用户角色共用）。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdListRequest {
    /// 目标 id 列表（全量覆盖语义）。
    pub ids: Vec<Uuid>,
}

/// 查询角色已绑定的权限点 id。
#[utoipa::path(
    get,
    path = "/api/system/roles/{id}/permissions",
    tag = "system",
    params(("id" = Uuid, Path, description = "角色 id")),
    responses((status = 200, description = "已绑定的权限点 id 列表", body = ApiResponse<Vec<Uuid>>))
)]
async fn get_role_permissions(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(role_id): Path<Uuid>,
) -> Result<ApiResponse<Vec<Uuid>>, AppError> {
    require_perm(&user, crate::perms::ROLE_LIST)?;
    ensure_role_in_tenant(&mut tx, user.tenant_id, role_id).await?;

    let ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT permission_id FROM sys.sys_role_permission WHERE role_id = $1",
    )
    .bind(role_id)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(ids))
}

/// 全量覆盖角色的权限点。
///
/// 采用"先删后插"而不是增量 diff：语义简单、不会出现半更新状态，
/// 且在单个事务内完成，失败自动回滚。
#[utoipa::path(
    put,
    path = "/api/system/roles/{id}/permissions",
    tag = "system",
    params(("id" = Uuid, Path, description = "角色 id")),
    request_body = IdListRequest,
    responses((status = 200, description = "保存后的权限点 id 列表", body = ApiResponse<Vec<Uuid>>))
)]
async fn set_role_permissions(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(role_id): Path<Uuid>,
    Json(req): Json<IdListRequest>,
) -> Result<ApiResponse<Vec<Uuid>>, AppError> {
    require_perm(&user, crate::perms::ROLE_EDIT)?;
    ensure_role_in_tenant(&mut tx, user.tenant_id, role_id).await?;

    // 权限点必须存在；且非平台超管只能授租户级权限。
    // 这是防「租户管理员给自己授 system:tenant:* 再越权管理所有租户」的关键校验。
    let mut wanted = req.ids.clone();
    wanted.sort_unstable();
    wanted.dedup();
    let grantable: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sys.sys_permission
         WHERE id = ANY($1) AND ($2::bool OR scope = 'tenant')",
    )
    .bind(&wanted)
    .bind(user.is_super_admin())
    .fetch_one(&mut *tx)
    .await?;
    if grantable != wanted.len() as i64 {
        return Err(AppError::Validation(
            "包含不存在或当前身份无权授予的权限点".into(),
        ));
    }

    let before: Vec<Uuid> = sqlx::query_scalar(
        "SELECT permission_id FROM sys.sys_role_permission WHERE role_id = $1 ORDER BY permission_id",
    )
    .bind(role_id)
    .fetch_all(&mut *tx)
    .await?;

    sqlx::query("DELETE FROM sys.sys_role_permission WHERE role_id = $1")
        .bind(role_id)
        .execute(&mut *tx)
        .await?;

    for permission_id in &wanted {
        sqlx::query(
            "INSERT INTO sys.sys_role_permission (role_id, permission_id) VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(role_id)
        .bind(permission_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // 权限点变更立即生效：吊销持有该角色的所有用户会话
    match state
        .sessions
        .revoke_role_sessions(user.tenant_id, role_id)
        .await
    {
        Ok(n) if n > 0 => {
            tracing::info!(role_id = %role_id, revoked = n, "角色权限变更，已撤销相关会话");
        }
        Ok(_) => {}
        Err(e) => tracing::error!(error = %e, role_id = %role_id, "撤销角色会话失败"),
    }

    crate::audit::record_user(
        &state.db,
        &user,
        "system.role.permissions",
        "sys_role",
        Some(&role_id.to_string()),
        Some(serde_json::json!({ "before": before, "after": req.ids })),
    )
    .await;

    tracing::info!(role_id = %role_id, count = req.ids.len(), "角色权限已更新");
    Ok(ApiResponse::new(req.ids))
}
