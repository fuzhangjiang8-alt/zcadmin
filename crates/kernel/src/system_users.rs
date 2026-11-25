//! 用户接口：列表、创建、状态变更与用户-角色绑定。
//!
//! 边界：角色与权限点在 `system_identity`；部门校验由 `system_identity` 提供。
//! 关键约束：写接口必须校验目标用户属于当前租户（sys_user_role 无 RLS 兜底）。

use axum::extract::{Path, Query, State};
use axum::routing::{get, patch};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::{ApiResponse, Page, PageMeta, PageQuery};
use crate::state::AppState;
use crate::system_identity::require_perm;
use crate::tenant::TenantTx;

/// 用户路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users))
        .route("/users/{id}/status", patch(update_user_status))
        .merge(crate::system_users_create::router())
        .merge(crate::system_user_roles::router())
}

/// 用户视图对象（对外暴露的字段白名单）。
///
/// **绝不直接序列化数据库行**：`password_hash` 这类字段一旦泄漏就是事故。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserVo {
    /// 用户 id。
    pub id: Uuid,
    /// 登录名。
    pub username: String,
    /// 显示名。
    pub display_name: String,
    /// 邮箱（无敏感字段权限时已脱敏）。
    pub email: Option<String>,
    /// 手机号（无敏感字段权限时已脱敏）。
    pub phone: Option<String>,
    /// 所属部门 id（可能为空）。
    pub dept_id: Option<Uuid>,
    /// 所属部门名称（可能为空）。
    pub dept_name: Option<String>,
    /// 岗位名称（多个用顿号连接，可能为空）。
    pub post_names: Option<String>,
    /// 状态：active / locked / disabled。
    pub status: String,
    /// 是否超级管理员。
    pub is_super: bool,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 用户列表查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UserQuery {
    /// 分页参数。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 关键字：匹配用户名或显示名（可选）。
    pub keyword: Option<String>,
}

/// 用户列表。
#[utoipa::path(
    get,
    path = "/api/system/users",
    tag = "system",
    params(
        ("page" = Option<u64>, Query, description = "页码，从 1 开始"),
        ("size" = Option<u64>, Query, description = "每页条数，最大 200"),
        ("keyword" = Option<String>, Query, description = "用户名或显示名模糊匹配")
    ),
    responses((status = 200, description = "用户分页列表", body = ApiResponse<Page<UserVo>>))
)]
async fn list_users(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(query): Query<UserQuery>,
) -> Result<ApiResponse<Page<UserVo>>, AppError> {
    require_perm(&user, crate::perms::USER_LIST)?;
    let keyword = query
        .keyword
        .as_deref()
        .map(|s| format!("%{}%", s.trim()))
        .filter(|s| s != "%%");

    // 数据权限：复用内核唯一实现 kernel::scope::condition_with_owner。
    let list_sql = format!(
        r#"
        SELECT u.id, u.username, u.display_name, u.email, u.phone, u.dept_id,
               u.status, u.is_super, u.created_at,
               d.name AS dept_name,
               (SELECT string_agg(p.name, '、' ORDER BY p.sort)
                FROM sys.sys_user_post up JOIN sys.sys_post p ON p.id = up.post_id
                WHERE up.user_id = u.id) AS post_names
        FROM sys.sys_user u
        LEFT JOIN sys.sys_dept d ON d.id = u.dept_id
        WHERE u.deleted_at IS NULL
          AND ($1::text IS NULL OR u.username ILIKE $1 OR u.display_name ILIKE $1)
          {}
        ORDER BY u.created_at DESC, u.id
        LIMIT $2 OFFSET $3
        "#,
        crate::scope::condition_with_owner("u.id", "u.dept_id", "$4", "$5", "$6")
    );
    let rows = sqlx::query_as::<_, UserVo>(&list_sql)
        .bind(keyword.as_deref())
        .bind(query.page.limit())
        .bind(query.page.offset())
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_all(&mut *tx)
        .await?;

    let count_sql = format!(
        r#"
        SELECT count(*)
        FROM sys.sys_user u
        WHERE u.deleted_at IS NULL
          AND ($1::text IS NULL OR u.username ILIKE $1 OR u.display_name ILIKE $1)
          {}
        "#,
        crate::scope::condition_with_owner("u.id", "u.dept_id", "$2", "$3", "$4")
    );
    let total: i64 = sqlx::query_scalar(&count_sql)
        .bind(keyword.as_deref())
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_one(&mut *tx)
        .await?;

    // 字段级脱敏：没有 view_sensitive 权限的人只能看到打码后的邮箱
    let can_view_sensitive = user.has_permission(crate::perms::USER_VIEW_SENSITIVE);
    let data = rows
        .into_iter()
        .map(|mut u| {
            if !can_view_sensitive {
                u.email = u.email.as_deref().map(crate::mask::email);
                u.phone = u.phone.as_deref().map(crate::mask::phone);
            }
            u
        })
        .collect();

    tx.commit().await?;
    Ok(ApiResponse::new(Page {
        data,
        page: PageMeta::new(query.page.page(), query.page.size(), total as u64),
    }))
}

/// 修改用户状态请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusRequest {
    /// 目标状态：active / locked / disabled。
    pub status: String,
}

/// 启用 / 锁定 / 禁用用户。
#[utoipa::path(
    patch,
    path = "/api/system/users/{id}/status",
    tag = "system",
    params(("id" = Uuid, Path, description = "用户 id")),
    request_body = UpdateStatusRequest,
    responses((status = 200, description = "更新后的用户", body = ApiResponse<UserVo>))
)]
async fn update_user_status(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateStatusRequest>,
) -> Result<ApiResponse<UserVo>, AppError> {
    require_perm(&user, crate::perms::USER_EDIT)?;
    if !matches!(req.status.as_str(), "active" | "locked" | "disabled") {
        return Err(AppError::Validation("非法的状态值".into()));
    }
    // 不允许把自己禁用：会导致当前会话立即失效且无人可救
    if id == user.user_id && req.status != "active" {
        return Err(AppError::Validation("不能停用当前登录账号".into()));
    }

    let before_status: Option<String> = sqlx::query_scalar(
        "SELECT status FROM sys.sys_user WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;

    let vo = sqlx::query_as::<_, UserVo>(
        r#"
        UPDATE sys.sys_user AS u
        SET status = $1, updated_at = now(), updated_by = $2, version = u.version + 1
        WHERE u.id = $3 AND u.tenant_id = $4 AND u.deleted_at IS NULL
        RETURNING u.id, u.username, u.display_name, u.email, u.phone, u.dept_id,
                  u.status, u.is_super, u.created_at,
                  (SELECT d.name FROM sys.sys_dept d WHERE d.id = u.dept_id) AS dept_name,
                  (SELECT string_agg(p.name, '、' ORDER BY p.sort)
                   FROM sys.sys_user_post up JOIN sys.sys_post p ON p.id = up.post_id
                   WHERE up.user_id = u.id) AS post_names
        "#,
    )
    .bind(&req.status)
    .bind(user.user_id)
    .bind(id)
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    tx.commit().await?;

    // 禁用/锁定必须立刻踢下线：会话里缓存了权限快照，
    // 只改 status 不会让已登录的会话失效（最长 8 小时）。
    if vo.status != "active" {
        match state
            .sessions
            .revoke_user_sessions(user.tenant_id, vo.id, None)
            .await
        {
            Ok(n) if n > 0 => {
                tracing::info!(user_id = %vo.id, status = %vo.status, revoked = n, "用户状态变更，已撤销会话");
            }
            Ok(_) => {}
            Err(e) => tracing::error!(error = %e, user_id = %vo.id, "撤销用户会话失败"),
        }
        // 令牌是独立凭据，不受会话撤销影响，必须单独吊销。
        match crate::auth_tokens::revoke_user_tokens(&state.db, user.tenant_id, vo.id).await {
            Ok(n) if n > 0 => {
                tracing::info!(user_id = %vo.id, revoked_tokens = n, "用户停用，已吊销访问令牌");
            }
            Ok(_) => {}
            Err(e) => tracing::error!(error = %e, user_id = %vo.id, "吊销访问令牌失败"),
        }
    }

    crate::audit::record_user(
        &state.db,
        &user,
        "system.user.status",
        "sys_user",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({
            "before": { "status": before_status },
            "after": { "status": vo.status }
        })),
    )
    .await;

    Ok(ApiResponse::new(vo))
}

// =============================================================================
// 角色与权限点
// =============================================================================
