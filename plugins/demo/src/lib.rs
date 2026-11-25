//! 示例插件（`demo`）：最小可运行黄金样例（实现 `Plugin` 契约、独立 schema 迁移、RLS 隔离与 OpenAPI）。
//!
//! 边界：仅演示单实体 CRUD 与权限校验，复杂业务插件请参考 `plugins/crm` 或 `plugins/workflow` 分模块拆分。

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use axum::routing::get;
use chrono::{DateTime, Utc};
use kernel::prelude::*;
use kernel::response::{Page, PageMeta, PageQuery};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::OpenApi;

const PLUGIN_ID: &str = "demo";
const PLUGIN_NAME: &str = "示例插件";
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// 示例实体（对应 `demo.demo_item`）。
#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct DemoItem {
    /// 主键（UUIDv7，时间有序）。
    pub id: Uuid,
    /// 名称。
    pub name: String,
    /// 备注（可空）。
    pub note: Option<String>,
    /// 创建时间（UTC）。
    pub created_at: DateTime<Utc>,
}

/// 创建请求体（独立于响应实体以防止 mass assignment）。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateItemRequest {
    /// 名称，1~64 字符。
    pub name: String,
    /// 备注，最长 500 字符。
    pub note: Option<String>,
}

/// 插件实现。
pub struct DemoPlugin;

impl Plugin for DemoPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta {
            id: PLUGIN_ID,
            name: PLUGIN_NAME,
            version: PLUGIN_VERSION,
            depends_on: &[],
            order: 100,
        }
    }

    fn migrations(&self) -> Option<&'static sqlx::migrate::Migrator> {
        Some(&MIGRATOR)
    }

    fn routes(&self) -> Router<AppState> {
        router()
    }

    fn openapi(&self) -> Option<utoipa::openapi::OpenApi> {
        Some(DemoApi::openapi())
    }

    fn permissions(&self) -> Vec<kernel::plugin::PermissionDef> {
        vec![
            kernel::plugin::PermissionDef {
                code: "demo:item:list",
                name: "示例数据查询",
            },
            kernel::plugin::PermissionDef {
                code: "demo:item:add",
                name: "示例数据新增",
            },
            kernel::plugin::PermissionDef {
                code: "demo:item:delete",
                name: "示例数据删除",
            },
        ]
    }
}

/// 构造本插件路由表（挂载到 `/api/demo`）。
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ping", get(ping))
        .route("/items", get(list_items).post(create_item))
        .route("/items/{id}", get(get_item).delete(delete_item))
}

/// 健康探测：确认插件路由已挂载。
///
/// 仍然要求登录：插件里没有任何「公开接口」的先例，
/// 一个匿名可打的端点会被当成存活探测器的放大面（或信息收集入口）。
async fn ping(_user: CurrentUser) -> ApiResult<&'static str> {
    Ok(ApiResponse::new("pong"))
}

/// 分页查询示例。
///
/// 演示三件事：参数归一化（[`PageQuery`]）、参数化 SQL、统一分页响应。
#[utoipa::path(
    get,
    path = "/api/demo/items",
    tag = "demo",
    params(
        ("page" = Option<u64>, Query, description = "页码"),
        ("size" = Option<u64>, Query, description = "每页条数")
    ),
    responses((status = 200, description = "分页列表", body = ApiResponse<Page<DemoItem>>))
)]
async fn list_items(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(query): Query<PageQuery>,
) -> ApiResult<Page<DemoItem>> {
    if !user.has_permission("demo:item:list") {
        return Err(AppError::Forbidden);
    }
    let items = sqlx::query_as::<_, DemoItem>(
        "SELECT id, name, note, created_at FROM demo.demo_item
         WHERE tenant_id = $1 AND deleted_at IS NULL
         ORDER BY created_at DESC, id LIMIT $2 OFFSET $3",
    )
    .bind(user.tenant_id)
    .bind(query.limit())
    .bind(query.offset())
    .fetch_all(&mut *tx)
    .await?;

    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM demo.demo_item WHERE tenant_id = $1 AND deleted_at IS NULL",
    )
    .bind(user.tenant_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(ApiResponse::new(Page {
        data: items,
        page: PageMeta::new(query.page(), query.size(), total as u64),
    }))
}

/// 详情查询示例。
///
/// 不存在时返回 404；真实业务里越权也应返回 404（不泄漏资源是否存在）。
#[utoipa::path(
    get,
    path = "/api/demo/items/{id}",
    tag = "demo",
    params(("id" = Uuid, Path, description = "item id")),
    responses((status = 200, description = "详情", body = ApiResponse<DemoItem>))
)]
async fn get_item(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<DemoItem> {
    if !user.has_permission("demo:item:list") {
        return Err(AppError::Forbidden);
    }
    let item = sqlx::query_as::<_, DemoItem>(
        "SELECT id, name, note, created_at FROM demo.demo_item
         WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    tx.commit().await?;
    Ok(ApiResponse::new(item))
}

/// 创建示例。
///
/// 校验规则写在 handler 里只是 M0 的简化；正式业务应下沉到 `domain` 层，
/// 保证「同一规则在导入、API、任务里都生效」。
#[utoipa::path(
    post,
    path = "/api/demo/items",
    tag = "demo",
    request_body = CreateItemRequest,
    responses((status = 200, description = "创建成功", body = ApiResponse<DemoItem>))
)]
async fn create_item(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<CreateItemRequest>,
) -> ApiResult<DemoItem> {
    if !user.has_permission("demo:item:add") {
        return Err(AppError::Forbidden);
    }
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(AppError::Validation("name 长度必须为 1~64 个字符".into()));
    }
    if let Some(note) = &req.note {
        if note.chars().count() > 500 {
            return Err(AppError::Validation("note 最长 500 个字符".into()));
        }
    }

    let item = sqlx::query_as::<_, DemoItem>(
        "INSERT INTO demo.demo_item (id, tenant_id, name, note, created_by)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, name, note, created_at",
    )
    .bind(Uuid::now_v7())
    .bind(user.tenant_id)
    .bind(name)
    .bind(&req.note)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(item))
}

/// 删除示例（软删）。
///
/// 软删保留审计线索；物理删除由归档任务统一处理（见数据架构文档）。
#[utoipa::path(
    delete,
    path = "/api/demo/items/{id}",
    tag = "demo",
    params(("id" = Uuid, Path, description = "item id")),
    responses((status = 200, description = "删除成功"))
)]
async fn delete_item(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<()> {
    if !user.has_permission("demo:item:delete") {
        return Err(AppError::Forbidden);
    }
    let affected = sqlx::query(
        "UPDATE demo.demo_item SET deleted_at = now()
         WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
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
    Ok(ApiResponse::new(()))
}

#[derive(utoipa::OpenApi)]
#[openapi(
    paths(list_items, get_item, create_item, delete_item),
    components(schemas(DemoItem, CreateItemRequest))
)]
struct DemoApi;

inventory::submit! {
    kernel::plugin::PluginRegistration { create: || Box::new(DemoPlugin) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 元信息必须满足内核的命名约束（小写字母/数字/下划线）。
    #[test]
    fn meta_id_is_valid() {
        let meta = DemoPlugin.meta();
        assert!(meta
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
    }

    /// 路由前缀与插件 id 必须一致，否则会挂到错误路径。
    #[test]
    fn routes_prefix_matches_id() {
        assert_eq!(DemoPlugin.meta().id, "demo");
    }
}
