//! 新建租户接口。
//!
//! 边界：列表/状态/配额/删除/恢复在 `tenant_admin`；本模块只做创建。
//! 关键约束：创建时初始化管理员、默认菜单与计费配置，全程单事务。

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::password;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::{ensure_default_menus, require_perm, require_platform};
use crate::tenant::TenantTx;
use crate::tenant_admin::TenantVo;

/// 新建租户路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/tenants", post(create_tenant))
}

/// 创建租户请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateTenantRequest {
    /// 租户编码（小写字母/数字/-/_）。
    pub code: String,
    /// 租户名称。
    pub name: String,
    /// 初始管理员登录名。
    pub admin_username: String,
    /// 初始管理员显示名。
    pub admin_display_name: String,
    /// 初始管理员密码。
    pub admin_password: String,
    /// 用户数上限（0 不限制）。
    pub max_users: Option<i32>,
    /// 存储上限字节（0 不限制）。
    pub max_storage_bytes: Option<i64>,
}

/// 创建租户并初始化管理员。
#[utoipa::path(
    post,
    path = "/api/system/tenants",
    tag = "system",
    request_body = CreateTenantRequest,
    responses((status = 200, description = "创建成功", body = ApiResponse<TenantVo>))
)]
async fn create_tenant(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<CreateTenantRequest>,
) -> Result<ApiResponse<TenantVo>, AppError> {
    require_platform(&user)?;
    require_perm(&user, crate::perms::TENANT_ADD)?;
    let code = req.code.trim().to_lowercase();
    if code.len() < 2
        || code.len() > 64
        || !code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(AppError::Validation(
            "租户编码只能包含小写字母、数字、-、_".into(),
        ));
    }
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > 128 {
        return Err(AppError::Validation(
            "租户名称不能为空且不超过 128 字符".into(),
        ));
    }
    if req.admin_username.trim().len() < 3 {
        return Err(AppError::Validation("管理员登录名至少 3 位".into()));
    }
    password::validate_strength(&req.admin_password)
        .map_err(|e| AppError::Validation(e.to_string()))?;
    let max_users = req.max_users.unwrap_or(0);
    if !(0..=100_000).contains(&max_users) {
        return Err(AppError::Validation("用户数上限必须在 0-100000".into()));
    }
    let max_storage_bytes = req.max_storage_bytes.unwrap_or(0);
    if !(0..=1_000_000_000_000_000).contains(&max_storage_bytes) {
        return Err(AppError::Validation("存储上限超出范围".into()));
    }

    let tenant_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO sys.sys_tenant (id, code, name, status, max_users, max_storage_bytes)
         VALUES ($1, $2, $3, 'active', $4, $5)",
    )
    .bind(tenant_id)
    .bind(&code)
    .bind(name)
    .bind(max_users)
    .bind(max_storage_bytes)
    .execute(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            AppError::Validation("租户编码已存在".into())
        }
        other => AppError::Database(other),
    })?;
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await?;
    let hash = password::hash_password_async(req.admin_password.clone()).await?;
    let admin_user_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO sys.sys_user
            (id, tenant_id, username, display_name, password_hash, is_super, status,
             must_change_password)
         VALUES ($1, $2, $3, $4, $5, false, 'active', true)",
    )
    .bind(admin_user_id)
    .bind(tenant_id)
    .bind(req.admin_username.trim())
    .bind(req.admin_display_name.trim())
    .bind(hash)
    .execute(&mut *tx)
    .await?;

    // 租户管理员 = 租户内全部权限，但**绝不是平台超管**。
    // 为什么不能给 is_super：登录时 is_super 会展开为 `*`，
    // 同时 list_permissions 会返回平台级权限点，等于让租户管理员管理所有租户。
    let role_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO sys.sys_role
            (id, tenant_id, code, name, description, data_scope, is_system, created_by)
         VALUES ($1, $2, 'tenant_admin', '租户管理员', '租户内全部权限（不含平台级）', 'all', true, $3)",
    )
    .bind(role_id)
    .bind(tenant_id)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO sys.sys_role_permission (role_id, permission_id)
         SELECT $1, p.id FROM sys.sys_permission p
          WHERE p.scope = 'tenant' AND p.deprecated = false
         ON CONFLICT DO NOTHING",
    )
    .bind(role_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO sys.sys_user_role (user_id, role_id) VALUES ($1, $2)")
        .bind(admin_user_id)
        .bind(role_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    // 新租户补齐默认菜单（幂等，覆盖所有租户）
    if let Err(e) = ensure_default_menus(&state.db).await {
        tracing::warn!(error = %e, tenant = %tenant_id, "初始化租户菜单失败");
    }

    crate::audit::record_user(
        &state.db,
        &user,
        "system.tenant.create",
        "sys_tenant",
        Some(&tenant_id.to_string()),
        Some(serde_json::json!({ "code": code, "name": name })),
    )
    .await;

    Ok(ApiResponse::new(TenantVo {
        id: tenant_id,
        code,
        name: name.to_string(),
        status: "active".into(),
        user_count: 1,
        max_users,
        max_storage_bytes,
        storage_used: 0,
        created_at: Utc::now(),
    }))
}
