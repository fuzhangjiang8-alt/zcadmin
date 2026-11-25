//! 角色与权限点查询/创建接口。
//!
//! 边界：角色授权在 `system_role_permissions`；用户绑定在 `system_user_roles`。
//! 关键约束：平台级权限点只对平台超管可见。

use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system_identity::require_perm;
use crate::tenant::TenantTx;

/// 角色路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/roles", get(list_roles).post(create_role))
        .route("/roles/{id}", put(update_role).delete(delete_role))
}

/// 角色视图对象。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RoleVo {
    /// 角色 id。
    pub id: Uuid,
    /// 角色编码，例如 super_admin。
    pub code: String,
    /// 角色名称。
    pub name: String,
    /// 描述。
    pub description: Option<String>,
    /// 数据范围：all / dept_and_child / custom / dept / self。
    pub data_scope: String,
    /// 是否内置角色。
    pub is_system: bool,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 角色列表（当前租户）。
#[utoipa::path(
    get,
    path = "/api/system/roles",
    tag = "system",
    responses((status = 200, description = "角色列表", body = ApiResponse<Vec<RoleVo>>))
)]
async fn list_roles(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<Vec<RoleVo>>, AppError> {
    require_perm(&user, crate::perms::ROLE_LIST)?;
    let rows = sqlx::query_as::<_, RoleVo>(
        "SELECT id, code, name, description, data_scope, is_system, created_at
         FROM sys.sys_role WHERE tenant_id = $1 AND deleted_at IS NULL ORDER BY is_system DESC, created_at",
    )
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 创建角色请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoleRequest {
    /// 角色编码，租户内唯一，例如 sales_manager。
    pub code: String,
    /// 角色名称。
    pub name: String,
    /// 描述。
    pub description: Option<String>,
    /// 数据范围，默认 self（最小权限）。
    #[serde(default = "default_data_scope")]
    pub data_scope: String,
}

/// 数据范围默认值：self。
fn default_data_scope() -> String {
    "self".to_string()
}

/// 更新角色请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRoleRequest {
    /// 角色名称。
    pub name: String,
    /// 描述。
    pub description: Option<String>,
    /// 数据范围：all / dept_and_child / custom / dept / self。
    pub data_scope: String,
}

fn validate_data_scope(scope: &str) -> Result<(), AppError> {
    matches!(scope, "all" | "dept_and_child" | "custom" | "dept" | "self")
        .then_some(())
        .ok_or_else(|| AppError::Validation("非法的数据范围".into()))
}

/// 创建角色。
#[utoipa::path(
    post,
    path = "/api/system/roles",
    tag = "system",
    request_body = CreateRoleRequest,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<RoleVo>),
        (status = 409, description = "角色编码已存在")
    )
)]
pub(crate) async fn create_role(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<CreateRoleRequest>,
) -> Result<ApiResponse<RoleVo>, AppError> {
    require_perm(&user, crate::perms::ROLE_ADD)?;
    validate_data_scope(&req.data_scope)?;

    let code = req.code.trim();
    if code.len() < 2 || code.len() > 64 {
        return Err(AppError::Validation(
            "角色编码长度必须为 2~64 个字符".into(),
        ));
    }
    if req.name.trim().is_empty() {
        return Err(AppError::Validation("角色名称不能为空".into()));
    }

    let result = sqlx::query_as::<_, RoleVo>(
        "INSERT INTO sys.sys_role (id, tenant_id, code, name, description, data_scope, is_system, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, false, $7)
         RETURNING id, code, name, description, data_scope, is_system, created_at",
    )
    .bind(Uuid::now_v7())
    .bind(user.tenant_id)
    .bind(code)
    .bind(req.name.trim())
    .bind(&req.description)
    .bind(&req.data_scope)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await;

    let vo = match result {
        Ok(vo) => vo,
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            return Err(AppError::Conflict("角色编码已存在".into()))
        }
        Err(e) => return Err(AppError::Database(e)),
    };

    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.role.create",
        "sys_role",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "code": vo.code, "dataScope": vo.data_scope })),
    )
    .await;
    Ok(ApiResponse::new(vo))
}

/// 更新角色名称、描述与数据范围（变更数据范围后立即吊销相关在线会话）。
#[utoipa::path(
    put,
    path = "/api/system/roles/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "角色 id")),
    request_body = UpdateRoleRequest,
    responses((status = 200, description = "更新后的角色", body = ApiResponse<RoleVo>))
)]
pub(crate) async fn update_role(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<ApiResponse<RoleVo>, AppError> {
    require_perm(&user, crate::perms::ROLE_EDIT)?;
    if req.name.trim().is_empty() {
        return Err(AppError::Validation("角色名称不能为空".into()));
    }
    validate_data_scope(&req.data_scope)?;

    let existing: Option<(bool, String)> = sqlx::query_as(
        "SELECT is_system, data_scope FROM sys.sys_role WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (is_system, old_scope) = existing.ok_or(AppError::NotFound)?;
    if is_system && req.data_scope != old_scope {
        return Err(AppError::Validation("内置角色不允许修改数据范围".into()));
    }

    let vo = sqlx::query_as::<_, RoleVo>(
        "UPDATE sys.sys_role
         SET name = $3, description = $4, data_scope = $5, updated_at = now(), updated_by = $6, version = version + 1
         WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL
         RETURNING id, code, name, description, data_scope, is_system, created_at",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(req.name.trim())
    .bind(&req.description)
    .bind(&req.data_scope)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    if old_scope != vo.data_scope {
        let _ = state
            .sessions
            .revoke_role_sessions(user.tenant_id, id)
            .await;
    }
    crate::audit::record_user(
        &state.db,
        &user,
        "system.role.update",
        "sys_role",
        Some(&id.to_string()),
        Some(serde_json::json!({ "name": vo.name, "dataScope": vo.data_scope })),
    )
    .await;
    Ok(ApiResponse::new(vo))
}

/// 删除角色（软删；内置角色或仍绑定用户的角色禁止删除）。
#[utoipa::path(
    delete,
    path = "/api/system/roles/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "角色 id")),
    responses((status = 200, description = "删除成功"))
)]
pub(crate) async fn delete_role(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::ROLE_EDIT)?;
    let is_system: Option<bool> = sqlx::query_scalar(
        "SELECT is_system FROM sys.sys_role WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    if is_system.ok_or(AppError::NotFound)? {
        return Err(AppError::Validation("系统内置角色不可删除".into()));
    }
    let bound_users: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sys.sys_user_role ur
         JOIN sys.sys_user u ON u.id = ur.user_id
         WHERE ur.role_id = $1 AND u.tenant_id = $2 AND u.deleted_at IS NULL",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_one(&mut *tx)
    .await?;
    if bound_users > 0 {
        return Err(AppError::Conflict("仍有用户绑定该角色，请先解绑".into()));
    }

    sqlx::query("UPDATE sys.sys_role SET deleted_at = now() WHERE id = $1 AND tenant_id = $2")
        .bind(id)
        .bind(user.tenant_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.role.delete",
        "sys_role",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}
