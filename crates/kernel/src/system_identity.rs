//! 身份域共享校验与路由聚合。
//!
//! # 职责
//!
//! - 租户归属校验：`ensure_role_in_tenant` / `ensure_user_in_tenant` / `ensure_dept_in_tenant`；
//! - 权限校验：`require_perm`；
//! - 聚合用户、角色、角色授权三个子路由。
//!
//! 具体 handler 在 `system_users` / `system_users_create` / `system_user_roles` /
//! `system_roles` / `system_role_permissions`。
//!
//! # 边界
//!
//! - 不处理菜单/字典/部门本身（见 `system`）；
//! - 会话管理（在线列表/强制下线）仍在 `system`；
//! - 所有写接口必须有对应权限点，且目标用户/角色必须属于当前租户。
//!
//! # 关键约束
//!
//! 从 `system.rs` 拆出。`ensure_*_in_tenant` 被 `system` 的其他域复用，
//! 因此保持 `pub(crate)` 并由 `system` 重新导出。

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::state::AppState;
use axum::Router;
use uuid::Uuid;

/// 用户/角色/权限路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .merge(crate::system_users::router())
        .merge(crate::system_roles::router())
        .merge(crate::system_role_permissions::router())
}

// =============================================================================
// 用户
// =============================================================================

/// 校验当前用户是否拥有某个权限点。
///
/// 目前是"函数调用式"校验（每个 handler 自己调），
/// 下一步会升级为路由层 `require_perm!` 宏，避免忘记调用。
pub(crate) fn require_perm(user: &CurrentUser, code: &str) -> Result<(), AppError> {
    if user.has_permission(code) {
        Ok(())
    } else {
        tracing::warn!(user = %user.username, perm = %code, "权限不足");
        Err(AppError::Forbidden)
    }
}

/// 校验调用者是**平台超管**（用于跨租户能力）。
///
/// # 为什么权限点不够
///
/// `system:tenant:*` 这类权限点的 `scope` 是 `platform`，正常渠道授不出去
/// （[`crate::system_role_permissions`] 会拒绝非超管授予）。但「只靠权限点」
/// 意味着任何一次数据订正、导入或迁移把平台权限点挂到租户角色上，
/// 租户管理员就能立刻管理/停用/删除**所有**租户。
/// 这里再加一道身份断言，成本一行，把「数据订正失误」与「越权」隔开。
pub(crate) fn require_platform(user: &CurrentUser) -> Result<(), AppError> {
    if user.is_super_admin() {
        Ok(())
    } else {
        tracing::warn!(
            user = %user.username,
            tenant = %user.tenant_id,
            "平台级接口拒绝非超管调用"
        );
        Err(AppError::Forbidden)
    }
}

// =============================================================================
// 角色：创建与授权
// =============================================================================

/// 校验角色存在且属于当前租户（防止跨租户授权）。
pub(crate) async fn ensure_role_in_tenant(
    conn: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    role_id: Uuid,
) -> Result<(), AppError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sys.sys_role WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL)",
    )
    .bind(role_id)
    .bind(tenant_id)
    .fetch_one(&mut *conn)
    .await?;

    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

/// 校验目标用户属于当前租户。
///
/// 为什么必须显式校验：`sys_user_role` 是无 tenant_id 的关联表，
/// 数据库层没有 RLS 兜底；只按 user_id 写入会允许跨租户改角色。
pub(crate) async fn ensure_user_in_tenant(
    conn: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<(), AppError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sys.sys_user
                       WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL)",
    )
    .bind(user_id)
    .bind(tenant_id)
    .fetch_one(&mut *conn)
    .await?;

    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

/// 校验部门属于当前租户。
///
/// 用于所有接受 dept_id 的写接口（建用户、建子部门、角色自定义范围），
/// 防止通过已知 UUID 构造跨租户的引用关系。
pub(crate) async fn ensure_dept_in_tenant(
    conn: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    dept_id: Uuid,
) -> Result<(), AppError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sys.sys_dept
                       WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL)",
    )
    .bind(dept_id)
    .bind(tenant_id)
    .fetch_one(&mut *conn)
    .await?;

    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

// =============================================================================
// 用户：角色绑定
// =============================================================================

/// 用户/角色/权限接口的 OpenAPI 片段。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        crate::system_users::list_users,
        crate::system_users_create::create_user,
        crate::system_users_create::update_user,
        crate::system_users::update_user_status,
        crate::system_roles::list_roles,
        crate::system_roles::create_role,
        crate::system_roles::update_role,
        crate::system_roles::delete_role,
        crate::system_role_permissions::list_permissions,
        crate::system_role_permissions::get_role_permissions,
        crate::system_role_permissions::set_role_permissions,
        crate::system_user_roles::get_user_roles,
        crate::system_user_roles::set_user_roles
    ),
    components(schemas(
        crate::system_users::UserVo,
        crate::system_users::UserQuery,
        crate::system_users_create::CreateUserRequest,
        crate::system_users_create::UpdateUserRequest,
        crate::system_users::UpdateStatusRequest,
        crate::system_roles::RoleVo,
        crate::system_role_permissions::PermissionVo,
        crate::system_roles::CreateRoleRequest,
        crate::system_roles::UpdateRoleRequest,
        crate::system_role_permissions::IdListRequest
    ))
)]
pub struct IdentityApi;
