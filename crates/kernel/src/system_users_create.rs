//! 新建用户接口。
//!
//! 边界：列表/状态在 `system_users`，用户-角色在 `system_user_roles`。
//! 关键约束：创建时校验租户用户配额、部门归属与密码策略。

use axum::extract::{Path, State};
use axum::routing::{post, put};
use axum::{Json, Router};
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::password;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system_identity::{ensure_dept_in_tenant, require_perm};
use crate::system_users::UserVo;
use crate::tenant::TenantTx;

/// 新建与编辑用户路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/users", post(create_user))
        .route("/users/{id}", put(update_user))
}

/// 编辑用户资料请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    /// 显示名（必填）。
    pub display_name: String,
    /// 邮箱（可选，传空字符串或 null 清空）。
    pub email: Option<String>,
    /// 手机号（可选，传空字符串或 null 清空）。
    pub phone: Option<String>,
    /// 所属部门（可选）。
    pub dept_id: Option<Uuid>,
}

/// 创建用户请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    /// 登录名，租户内唯一。
    pub username: String,
    /// 显示名。
    pub display_name: String,
    /// 初始密码，必须满足强度要求。
    pub password: String,
    /// 邮箱（可选）。
    pub email: Option<String>,
    /// 手机号（可选）。
    pub phone: Option<String>,
    /// 所属部门（可选，不传则不属于任何部门）。
    pub dept_id: Option<Uuid>,
    /// 是否要求首次登录修改密码。
    pub must_change_password: Option<bool>,
}

/// 创建用户。
#[utoipa::path(
    post,
    path = "/api/system/users",
    tag = "system",
    request_body = CreateUserRequest,
    responses(
        (status = 200, description = "创建成功", body = ApiResponse<UserVo>),
        (status = 409, description = "用户名已存在")
    )
)]
async fn create_user(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<CreateUserRequest>,
) -> Result<ApiResponse<UserVo>, AppError> {
    require_perm(&user, crate::perms::USER_ADD)?;
    let max_users: i32 = sqlx::query_scalar("SELECT max_users FROM sys.sys_tenant WHERE id = $1")
        .bind(user.tenant_id)
        .fetch_one(&mut *tx)
        .await?;
    let mut quota_used: Option<i64> = None;
    if max_users > 0 {
        let used: i64 = sqlx::query_scalar(
            "SELECT count(*)::bigint FROM sys.sys_user
             WHERE tenant_id = $1 AND deleted_at IS NULL",
        )
        .bind(user.tenant_id)
        .fetch_one(&mut *tx)
        .await?;
        quota_used = Some(used);
        if used >= i64::from(max_users) {
            return Err(AppError::Validation("租户用户数已达上限".into()));
        }
    }
    let username = req.username.trim();
    if username.len() < 3 || username.len() > 64 {
        return Err(AppError::Validation("用户名长度必须为 3~64 个字符".into()));
    }
    if req.display_name.trim().is_empty() {
        return Err(AppError::Validation("显示名不能为空".into()));
    }
    // 密码强度不达标直接拒绝，避免"弱密码靠事后治理"
    let policy = password::load_policy(&mut tx, user.tenant_id).await?;
    policy
        .validate(&req.password)
        .map_err(|e| AppError::Validation(e.to_string()))?;

    let phone = match req
        .phone
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(value) => Some(
            crate::mask::normalize_phone(value)
                .ok_or_else(|| AppError::Validation("手机号格式不正确".into()))?,
        ),
        None => None,
    };

    // Argon2 走受控阻塞池：避免建用户接口在并发下卡住 tokio worker。
    let hash = password::hash_password_async(req.password.clone()).await?;

    if let Some(dept_id) = req.dept_id {
        ensure_dept_in_tenant(&mut tx, user.tenant_id, dept_id).await?;
    }

    let result = sqlx::query_as::<_, UserVo>(
        r#"
        INSERT INTO sys.sys_user
            (id, tenant_id, username, display_name, password_hash, email, phone, dept_id, status,
             created_by, must_change_password)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'active', $9, $10)
        RETURNING id, username, display_name, email, phone, dept_id,
                  (SELECT name FROM sys.sys_dept WHERE id = $8) AS dept_name,
                  NULL::text AS post_names, status, is_super, created_at
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(user.tenant_id)
    .bind(username)
    .bind(req.display_name.trim())
    .bind(hash)
    .bind(&req.email)
    .bind(&phone)
    .bind(req.dept_id)
    .bind(user.user_id)
    .bind(req.must_change_password.unwrap_or(false))
    .fetch_one(&mut *tx)
    .await;

    let vo = match result {
        Ok(vo) => vo,
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            // 23505 = unique_violation：转成 409，而不是 500
            return Err(AppError::Conflict("用户名已存在".into()));
        }
        Err(e) => return Err(AppError::Database(e)),
    };
    tx.commit().await?;

    // 审计：记录谁在什么时候创建了哪个用户（不含密码）
    // 放在提交之后：审计失败不影响业务结果，也不能记录一个未提交的事件
    crate::audit::record_user(
        &state.db,
        &user,
        "system.user.create",
        "sys_user",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "username": vo.username })),
    )
    .await;
    if let Some(used) = quota_used {
        if (used + 1) * 5 >= i64::from(max_users) * 4 {
            let _ = crate::audit::record_alert(
                &state.db,
                user.tenant_id,
                "system.tenant.quota.user",
                used + 1,
                max_users,
            )
            .await;
        }
    }

    Ok(ApiResponse::new(vo))
}

/// 编辑用户基本资料（显示名、邮箱、手机号、所属部门；变更部门后自动刷新会话数据范围）。
#[utoipa::path(
    put,
    path = "/api/system/users/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "用户 id")),
    request_body = UpdateUserRequest,
    responses((status = 200, description = "更新后的用户", body = ApiResponse<UserVo>))
)]
pub(crate) async fn update_user(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateUserRequest>,
) -> Result<ApiResponse<UserVo>, AppError> {
    require_perm(&user, crate::perms::USER_EDIT)?;
    if req.display_name.trim().is_empty() {
        return Err(AppError::Validation("显示名不能为空".into()));
    }
    let phone = match req
        .phone
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(v) => Some(
            crate::mask::normalize_phone(v)
                .ok_or_else(|| AppError::Validation("手机号格式不正确".into()))?,
        ),
        None => None,
    };
    let email = req
        .email
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned);
    if let Some(dept_id) = req.dept_id {
        ensure_dept_in_tenant(&mut tx, user.tenant_id, dept_id).await?;
    }

    let old_dept: Option<Option<Uuid>> = sqlx::query_scalar(
        "SELECT dept_id FROM sys.sys_user WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    let old_dept = old_dept.ok_or(AppError::NotFound)?;

    let vo = sqlx::query_as::<_, UserVo>(
        "UPDATE sys.sys_user AS u
         SET display_name = $3, email = $4, phone = $5, dept_id = $6,
             updated_at = now(), updated_by = $7, version = u.version + 1
         WHERE u.id = $1 AND u.tenant_id = $2 AND u.deleted_at IS NULL
         RETURNING u.id, u.username, u.display_name, u.email, u.phone, u.dept_id,
                   u.status, u.is_super, u.created_at,
                   (SELECT d.name FROM sys.sys_dept d WHERE d.id = u.dept_id) AS dept_name,
                   (SELECT string_agg(p.name, '、' ORDER BY p.sort)
                    FROM sys.sys_user_post up JOIN sys.sys_post p ON p.id = up.post_id
                    WHERE up.user_id = u.id) AS post_names",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(req.display_name.trim())
    .bind(email)
    .bind(phone)
    .bind(req.dept_id)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    if old_dept != req.dept_id {
        let _ = state
            .sessions
            .revoke_user_sessions(user.tenant_id, id, None)
            .await;
    }
    crate::audit::record_user(
        &state.db,
        &user,
        "system.user.update",
        "sys_user",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "displayName": vo.display_name, "deptId": req.dept_id })),
    )
    .await;
    Ok(ApiResponse::new(vo))
}
