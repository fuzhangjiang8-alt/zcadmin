//! 参数配置（单个键值对控制运行行为，`is_public = false` 的参数不通过公开接口下发）。

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::extract::Query;
use crate::response::{ApiResponse, Page, PageMeta, PageQuery};
use crate::state::AppState;
use crate::system::require_perm;
use crate::tenant::TenantTx;
use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;
use uuid::Uuid;

/// 配置视图对象。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigVo {
    /// 配置 id。
    pub id: Uuid,
    /// 配置键，租户内唯一。
    pub key: String,
    /// 配置值（统一存字符串，由使用方解析）。
    pub value: String,
    /// 展示名称。
    pub name: String,
    /// 备注。
    pub remark: Option<String>,
    /// 是否允许前端读取。
    pub is_public: bool,
    /// 更新时间。
    pub updated_at: Option<DateTime<Utc>>,
}

/// 列表查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigQuery {
    /// 分页参数。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 键或名称模糊匹配。
    pub keyword: Option<String>,
}

/// 新建配置请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateConfigRequest {
    /// 配置键。
    pub key: String,
    /// 配置值。
    pub value: String,
    /// 展示名称。
    pub name: String,
    /// 备注。
    pub remark: Option<String>,
    /// 是否允许前端读取。
    pub is_public: Option<bool>,
}

/// 更新配置请求（键不可改）。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConfigRequest {
    /// 配置值。
    pub value: String,
    /// 展示名称。
    pub name: Option<String>,
    /// 备注。
    pub remark: Option<String>,
    /// 是否允许前端读取。
    pub is_public: Option<bool>,
}

/// 路由。
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/system/configs", get(list_configs).post(create_config))
        .route("/api/system/configs/public", get(public_configs))
        .route(
            "/api/system/configs/{id}",
            put(update_config).delete(delete_config),
        )
}

/// 配置列表。
#[utoipa::path(
    get,
    path = "/api/system/configs",
    tag = "system",
    params(("page" = Option<u64>, Query, description = "页码"), ("size" = Option<u64>, Query, description = "每页条数"), ("keyword" = Option<String>, Query, description = "键或名称模糊匹配")),
    responses((status = 200, description = "配置分页列表", body = ApiResponse<Page<ConfigVo>>))
)]
async fn list_configs(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<ConfigQuery>,
) -> Result<ApiResponse<Page<ConfigVo>>, AppError> {
    require_perm(&user, crate::perms::CONFIG_LIST)?;
    let keyword = q
        .keyword
        .as_deref()
        .map(|v| format!("%{}%", v.trim()))
        .filter(|v| v != "%%");
    let rows = sqlx::query_as::<_, ConfigVo>(
        "SELECT id, key, value, name, remark, is_public, updated_at FROM sys.sys_config
         WHERE tenant_id = $1 AND deleted_at IS NULL AND ($2::text IS NULL OR key ILIKE $2 OR name ILIKE $2)
         ORDER BY key LIMIT $3 OFFSET $4",
    )
    .bind(user.tenant_id)
    .bind(keyword.as_deref())
    .bind(q.page.limit())
    .bind(q.page.offset())
    .fetch_all(&mut *tx)
    .await?;
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sys.sys_config WHERE tenant_id = $1 AND deleted_at IS NULL
         AND ($2::text IS NULL OR key ILIKE $2 OR name ILIKE $2)",
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

/// 新建配置。
#[utoipa::path(
    post,
    path = "/api/system/configs",
    tag = "system",
    request_body = CreateConfigRequest,
    responses((status = 200, description = "创建成功", body = ApiResponse<ConfigVo>))
)]
async fn create_config(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<CreateConfigRequest>,
) -> Result<ApiResponse<ConfigVo>, AppError> {
    require_perm(&user, crate::perms::CONFIG_EDIT)?;
    let key = req.key.trim();
    if key.is_empty() || key.len() > 128 {
        return Err(AppError::Validation("配置键长度必须为 1~128 个字符".into()));
    }
    if req.name.trim().is_empty() {
        return Err(AppError::Validation("配置名称不能为空".into()));
    }
    let vo = sqlx::query_as::<_, ConfigVo>(
        "INSERT INTO sys.sys_config (id, tenant_id, key, value, name, remark, is_public)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, key, value, name, remark, is_public, updated_at",
    )
    .bind(Uuid::now_v7())
    .bind(user.tenant_id)
    .bind(key)
    .bind(&req.value)
    .bind(req.name.trim())
    .bind(&req.remark)
    .bind(req.is_public.unwrap_or(false))
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            AppError::Conflict("配置键已存在".into())
        }
        other => AppError::Database(other),
    })?;
    tx.commit().await?;
    Ok(ApiResponse::new(vo))
}

/// 更新配置。
#[utoipa::path(
    put,
    path = "/api/system/configs/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "配置 id")),
    request_body = UpdateConfigRequest,
    responses((status = 200, description = "更新后的配置", body = ApiResponse<ConfigVo>))
)]
async fn update_config(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<ApiResponse<ConfigVo>, AppError> {
    require_perm(&user, crate::perms::CONFIG_EDIT)?;

    let vo = sqlx::query_as::<_, ConfigVo>(
        "UPDATE sys.sys_config SET value = $3, name = COALESCE($4, name), remark = COALESCE($5, remark),
                is_public = COALESCE($6, is_public), updated_at = now()
         WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL
         RETURNING id, key, value, name, remark, is_public, updated_at",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(&req.value)
    .bind(&req.name)
    .bind(&req.remark)
    .bind(req.is_public)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.config.update",
        "sys_config",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "key": vo.key })),
    )
    .await;
    Ok(ApiResponse::new(vo))
}

/// 删除参数配置（软删）。
#[utoipa::path(
    delete,
    path = "/api/system/configs/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "配置 id")),
    responses((status = 200, description = "删除成功"))
)]
async fn delete_config(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::CONFIG_EDIT)?;
    let key: Option<String> = sqlx::query_scalar(
        "UPDATE sys.sys_config SET deleted_at = now(), updated_at = now()
         WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL RETURNING key",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?;
    let key = key.ok_or(AppError::NotFound)?;
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.config.delete",
        "sys_config",
        Some(&id.to_string()),
        Some(serde_json::json!({ "key": key })),
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 前端可读的公开配置（登录即可访问）。
#[utoipa::path(
    get,
    path = "/api/system/configs/public",
    tag = "system",
    responses((status = 200, description = "公开配置键值对"))
)]
async fn public_configs(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<HashMap<String, String>>, AppError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM sys.sys_config WHERE tenant_id = $1 AND is_public = true AND deleted_at IS NULL",
    )
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(rows.into_iter().collect()))
}

/// OpenAPI 片段。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list_configs, create_config, update_config, delete_config, public_configs),
    components(schemas(ConfigVo, ConfigQuery, CreateConfigRequest, UpdateConfigRequest)),
    tags((name = "system", description = "系统管理：用户/角色/权限/菜单/字典"))
)]
pub struct ConfigApi;
