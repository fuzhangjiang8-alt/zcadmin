//! 站内通知（租户级广播，已读状态按用户记录在 `sys_notice_read`）。

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::extract::Query;
use crate::response::{ApiResponse, Page, PageMeta, PageQuery};
use crate::state::AppState;
use crate::tenant::TenantTx;
use axum::extract::{Path, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// 通知视图对象。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoticeVo {
    /// 通知 id。
    pub id: Uuid,
    /// 标题。
    pub title: String,
    /// 正文（纯文本，前端按纯文本渲染，禁止 v-html）。
    pub content: String,
    /// 级别：info / success / warning / error。
    pub level: String,
    /// 发布时间。
    pub published_at: DateTime<Utc>,
    /// 当前用户是否已读。
    pub is_read: bool,
}

/// 未读数。
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UnreadCount {
    /// 未读通知数量。
    pub count: i64,
}

/// 发布请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateNoticeRequest {
    /// 标题，1~200 字符。
    pub title: String,
    /// 正文。
    pub content: String,
    /// 级别，默认 info。
    pub level: Option<String>,
}

/// 通知路由。
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/system/notices", get(list_notices).post(create_notice))
        .route("/api/system/notices/unread-count", get(unread_count))
        .route("/api/system/notices/read-all", post(mark_all_read))
        .route("/api/system/notices/{id}/read", post(mark_read))
        .route("/api/system/notices/{id}", delete(delete_notice))
}

/// 通知列表（含当前用户已读状态）。
#[utoipa::path(
    get,
    path = "/api/system/notices",
    tag = "system",
    params(("page" = Option<u64>, Query, description = "页码"), ("size" = Option<u64>, Query, description = "每页条数")),
    responses((status = 200, description = "通知分页列表", body = ApiResponse<Page<NoticeVo>>))
)]
async fn list_notices(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(page): Query<PageQuery>,
) -> Result<ApiResponse<Page<NoticeVo>>, AppError> {
    let rows = sqlx::query_as::<_, NoticeVo>(
        "SELECT n.id, n.title, n.content, n.level, n.published_at, (r.read_at IS NOT NULL) AS is_read
         FROM sys.sys_notice n LEFT JOIN sys.sys_notice_read r ON r.notice_id = n.id AND r.user_id = $2
         WHERE n.tenant_id = $1 AND n.deleted_at IS NULL AND n.status = 'published'
         ORDER BY n.published_at DESC, n.id LIMIT $3 OFFSET $4",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .bind(page.limit())
    .bind(page.offset())
    .fetch_all(&mut *tx)
    .await?;
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sys.sys_notice WHERE tenant_id = $1 AND deleted_at IS NULL AND status = 'published'",
    )
    .bind(user.tenant_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(Page {
        data: rows,
        page: PageMeta::new(page.page(), page.size(), total as u64),
    }))
}

/// 未读数量（前端轮询这个接口）。
#[utoipa::path(
    get,
    path = "/api/system/notices/unread-count",
    tag = "system",
    responses((status = 200, description = "未读数量", body = ApiResponse<UnreadCount>))
)]
async fn unread_count(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<UnreadCount>, AppError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sys.sys_notice n
         WHERE n.tenant_id = $1 AND n.deleted_at IS NULL AND n.status = 'published'
           AND NOT EXISTS (SELECT 1 FROM sys.sys_notice_read r WHERE r.notice_id = n.id AND r.user_id = $2)",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(UnreadCount { count }))
}

/// 标记单条已读。
#[utoipa::path(
    post,
    path = "/api/system/notices/{id}/read",
    tag = "system",
    params(("id" = Uuid, Path, description = "通知 id")),
    responses((status = 200, description = "标记成功"))
)]
async fn mark_read(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    // 先校验通知属于当前租户，防止跨租户探测
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sys.sys_notice WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL)",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }

    sqlx::query(
        "INSERT INTO sys.sys_notice_read (notice_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(()))
}

/// 全部标记已读。
#[utoipa::path(
    post,
    path = "/api/system/notices/read-all",
    tag = "system",
    responses((status = 200, description = "标记成功"))
)]
async fn mark_all_read(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<()>, AppError> {
    sqlx::query(
        "INSERT INTO sys.sys_notice_read (notice_id, user_id)
         SELECT n.id, $2 FROM sys.sys_notice n
         WHERE n.tenant_id = $1 AND n.deleted_at IS NULL AND n.status = 'published'
         ON CONFLICT DO NOTHING",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(()))
}

/// 发布通知。
#[utoipa::path(
    post,
    path = "/api/system/notices",
    tag = "system",
    request_body = CreateNoticeRequest,
    responses((status = 200, description = "发布成功", body = ApiResponse<NoticeVo>))
)]
async fn create_notice(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<CreateNoticeRequest>,
) -> Result<ApiResponse<NoticeVo>, AppError> {
    if !user.has_permission(crate::perms::NOTICE_ADD) {
        return Err(AppError::Forbidden);
    }

    let title = req.title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(AppError::Validation("标题长度必须为 1~200 个字符".into()));
    }
    if req.content.trim().is_empty() {
        return Err(AppError::Validation("正文不能为空".into()));
    }
    let level = req.level.as_deref().unwrap_or("info");
    if !matches!(level, "info" | "success" | "warning" | "error") {
        return Err(AppError::Validation("非法的通知级别".into()));
    }

    let vo = sqlx::query_as::<_, NoticeVo>(
        "INSERT INTO sys.sys_notice (id, tenant_id, title, content, level, publisher_id)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, title, content, level, published_at, false AS is_read",
    )
    .bind(Uuid::now_v7())
    .bind(user.tenant_id)
    .bind(title)
    .bind(req.content.trim())
    .bind(level)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.notice.publish",
        "sys_notice",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "title": vo.title })),
    )
    .await;
    Ok(ApiResponse::new(vo))
}

/// 删除/撤回通知（软删，需 `system:notice:add` 权限）。
#[utoipa::path(
    delete,
    path = "/api/system/notices/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "通知 id")),
    responses((status = 200, description = "删除成功"))
)]
async fn delete_notice(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    if !user.has_permission(crate::perms::NOTICE_ADD) {
        return Err(AppError::Forbidden);
    }
    let affected = sqlx::query(
        "UPDATE sys.sys_notice SET deleted_at = now() WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user.tenant_id)
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
        "system.notice.delete",
        "sys_notice",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// OpenAPI 片段。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list_notices, unread_count, mark_read, mark_all_read, create_notice, delete_notice),
    components(schemas(NoticeVo, UnreadCount, CreateNoticeRequest)),
    tags((name = "system", description = "系统管理：用户/角色/权限/菜单/字典"))
)]
pub struct NoticeApi;
