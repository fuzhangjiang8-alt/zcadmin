//! 岗位管理。
//!
//! 岗位（post）与部门（dept）是两个正交维度：
//! - 部门决定数据权限的「行范围」；
//! - 岗位是职责标签，用于筛选、审批流、报表分组。
//!
//! 一个用户可以有多个岗位，因此使用 `sys_user_post` 多对多表。

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::extract::Query;
use crate::posts_types::{PostQuery, PostRequest, PostVo};
use crate::posts_user::{require_perm, validate_post};
use crate::response::{ApiResponse, Page, PageMeta};
use crate::state::AppState;
use crate::tenant::TenantTx;
use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use uuid::Uuid;

/// 路由。
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/system/posts", get(list_posts).post(create_post))
        .route(
            "/api/system/posts/{id}",
            put(update_post).delete(delete_post),
        )
        .merge(crate::posts_user::router())
}

/// 岗位列表。
#[utoipa::path(
    get,
    path = "/api/system/posts",
    tag = "system",
    params(
        ("page" = Option<u64>, Query, description = "页码"),
        ("size" = Option<u64>, Query, description = "每页条数"),
        ("keyword" = Option<String>, Query, description = "编码/名称模糊匹配")
    ),
    responses((status = 200, description = "岗位分页列表", body = ApiResponse<Page<PostVo>>))
)]
async fn list_posts(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<PostQuery>,
) -> Result<ApiResponse<Page<PostVo>>, AppError> {
    require_perm(&user, crate::perms::POST_LIST)?;

    let keyword = q
        .keyword
        .as_deref()
        .map(|v| format!("%{}%", v.trim()))
        .filter(|v| v != "%%");

    let rows = sqlx::query_as::<_, PostVo>(
        r#"
        SELECT id, code, name, sort, status, remark, created_at
        FROM sys.sys_post
        WHERE tenant_id = $1 AND deleted_at IS NULL
          AND ($2::text IS NULL OR code ILIKE $2 OR name ILIKE $2)
        ORDER BY sort, code
        LIMIT $3 OFFSET $4
        "#,
    )
    .bind(user.tenant_id)
    .bind(keyword.as_deref())
    .bind(q.page.limit())
    .bind(q.page.offset())
    .fetch_all(&mut *tx)
    .await?;

    let total: i64 = sqlx::query_scalar(
        r#"
        SELECT count(*) FROM sys.sys_post
        WHERE tenant_id = $1 AND deleted_at IS NULL
          AND ($2::text IS NULL OR code ILIKE $2 OR name ILIKE $2)
        "#,
    )
    .bind(user.tenant_id)
    .bind(keyword.as_deref())
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(Page {
        data: rows,
        page: PageMeta::new(q.page.page(), q.page.size(), total as u64),
    }))
}

/// 新建岗位。
#[utoipa::path(
    post,
    path = "/api/system/posts",
    tag = "system",
    request_body = PostRequest,
    responses((status = 200, description = "创建成功", body = ApiResponse<PostVo>))
)]
async fn create_post(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<PostRequest>,
) -> Result<ApiResponse<PostVo>, AppError> {
    require_perm(&user, crate::perms::POST_ADD)?;
    validate_post(&req)?;

    let result = sqlx::query_as::<_, PostVo>(
        r#"
        INSERT INTO sys.sys_post (id, tenant_id, code, name, sort, status, remark)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, code, name, sort, status, remark, created_at
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(user.tenant_id)
    .bind(req.code.trim())
    .bind(req.name.trim())
    .bind(req.sort.unwrap_or(0))
    .bind(req.status.as_deref().unwrap_or("active"))
    .bind(&req.remark)
    .fetch_one(&mut *tx)
    .await;

    let vo = match result {
        Ok(vo) => vo,
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            return Err(AppError::Conflict("岗位编码已存在".into()));
        }
        Err(e) => return Err(AppError::Database(e)),
    };
    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.post.create",
        "sys_post",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "code": vo.code })),
    )
    .await;

    Ok(ApiResponse::new(vo))
}

/// 更新岗位。
#[utoipa::path(
    put,
    path = "/api/system/posts/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "岗位 id")),
    request_body = PostRequest,
    responses((status = 200, description = "更新成功", body = ApiResponse<PostVo>))
)]
async fn update_post(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<PostRequest>,
) -> Result<ApiResponse<PostVo>, AppError> {
    require_perm(&user, crate::perms::POST_EDIT)?;
    validate_post(&req)?;

    let result = sqlx::query_as::<_, PostVo>(
        r#"
        UPDATE sys.sys_post
        SET code = $3, name = $4, sort = $5, status = $6, remark = $7
        WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL
        RETURNING id, code, name, sort, status, remark, created_at
        "#,
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(req.code.trim())
    .bind(req.name.trim())
    .bind(req.sort.unwrap_or(0))
    .bind(req.status.as_deref().unwrap_or("active"))
    .bind(&req.remark)
    .fetch_optional(&mut *tx)
    .await;

    let vo = match result {
        Ok(Some(vo)) => vo,
        Ok(None) => return Err(AppError::NotFound),
        Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
            return Err(AppError::Conflict("岗位编码已存在".into()));
        }
        Err(e) => return Err(AppError::Database(e)),
    };
    tx.commit().await?;

    crate::audit::record_user(
        &state.db,
        &user,
        "system.post.update",
        "sys_post",
        Some(&vo.id.to_string()),
        None,
    )
    .await;

    Ok(ApiResponse::new(vo))
}

/// 删除岗位（有用户绑定时拒绝）。
#[utoipa::path(
    delete,
    path = "/api/system/posts/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "岗位 id")),
    responses((status = 200, description = "删除成功"))
)]
async fn delete_post(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::POST_DELETE)?;

    let bound: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sys.sys_user_post WHERE post_id = $1)")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    if bound {
        return Err(AppError::Conflict("岗位下还有用户，不能删除".into()));
    }

    let affected = sqlx::query(
        "UPDATE sys.sys_post SET deleted_at = now() WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
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
        "system.post.delete",
        "sys_post",
        Some(&id.to_string()),
        None,
    )
    .await;

    Ok(ApiResponse::new(()))
}

/// OpenAPI 片段。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        list_posts,
        create_post,
        update_post,
        delete_post,
        crate::posts_user::get_user_posts,
        crate::posts_user::set_user_posts
    ),
    components(schemas(
        PostVo,
        PostQuery,
        PostRequest,
        crate::posts_user::UserPostRequest
    )),
    tags((name = "system", description = "系统管理：用户/角色/权限/菜单/字典"))
)]
pub struct PostApi;
