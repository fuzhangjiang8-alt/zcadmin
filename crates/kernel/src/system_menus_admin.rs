//! 菜单管理接口：管理员视角的平铺列表、新建、更新、删除。
//!
//! 边界：当前用户菜单树在 `system_menus`；插件菜单同步在 `system_menu_sync`。
//! 关键约束：父菜单必须同租户；有子菜单时禁止删除。

use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::require_perm;
use crate::system_menus::{ensure_menu_in_tenant, ensure_valid_parent};
use crate::tenant::TenantTx;

/// 菜单管理路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/menus", axum::routing::post(create_menu))
        .route("/menus/all", get(admin_list_menus))
        .route("/menus/{id}", put(update_menu).delete(delete_menu))
}

/// 菜单管理视图（平铺，前端自行组树）。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MenuAdminVo {
    /// 菜单 id、父菜单 id、标题、路由路径、图标、权限点、排序值、是否可见。
    pub id: Uuid,
    /// 父菜单 id。
    pub parent_id: Option<Uuid>,
    /// 标题。
    pub title: String,
    /// 路由路径。
    pub path: String,
    /// 图标。
    pub icon: Option<String>,
    /// 权限点。
    pub perm: Option<String>,
    /// 排序值。
    pub sort: i32,
    /// 是否可见。
    pub visible: bool,
}

/// 新建/更新菜单请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MenuRequest {
    /// 父菜单（为空表示根菜单）。
    pub parent_id: Option<Uuid>,
    /// 标题。
    pub title: String,
    /// 路由路径，必须以 / 开头。
    pub path: String,
    /// 图标名。
    pub icon: Option<String>,
    /// 需要的权限点。
    pub perm: Option<String>,
    /// 排序值。
    pub sort: Option<i32>,
    /// 是否可见。
    pub visible: Option<bool>,
}

/// 菜单管理列表（含隐藏菜单，平铺结构）。
#[utoipa::path(
    get,
    path = "/api/system/menus/all",
    tag = "system",
    responses((status = 200, description = "全部菜单（平铺）", body = ApiResponse<Vec<MenuAdminVo>>))
)]
pub(crate) async fn admin_list_menus(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<Vec<MenuAdminVo>>, AppError> {
    require_perm(&user, crate::perms::MENU_LIST)?;

    let rows = sqlx::query_as::<_, MenuAdminVo>(
        "SELECT id, parent_id, title, path, icon, perm, sort, visible
         FROM sys.sys_menu WHERE tenant_id = $1 ORDER BY sort, id",
    )
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(rows))
}

/// 新建菜单。
#[utoipa::path(
    post,
    path = "/api/system/menus",
    tag = "system",
    request_body = MenuRequest,
    responses((status = 200, description = "创建成功", body = ApiResponse<MenuAdminVo>))
)]
pub(crate) async fn create_menu(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<MenuRequest>,
) -> Result<ApiResponse<MenuAdminVo>, AppError> {
    require_perm(&user, crate::perms::MENU_ADD)?;
    validate_menu(&req)?;

    if let Some(parent_id) = req.parent_id {
        ensure_menu_in_tenant(&mut tx, user.tenant_id, parent_id).await?;
    }

    let vo = sqlx::query_as::<_, MenuAdminVo>(
        "INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort, visible)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING id, parent_id, title, path, icon, perm, sort, visible",
    )
    .bind(Uuid::now_v7())
    .bind(user.tenant_id)
    .bind(req.parent_id)
    .bind(req.title.trim())
    .bind(req.path.trim())
    .bind(&req.icon)
    .bind(&req.perm)
    .bind(req.sort.unwrap_or(0))
    .bind(req.visible.unwrap_or(true))
    .fetch_one(&mut *tx)
    .await
    .map_err(map_menu_db_err)?;

    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.menu.create",
        "sys_menu",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "path": vo.path })),
    )
    .await;
    Ok(ApiResponse::new(vo))
}

/// 更新菜单。
#[utoipa::path(
    put,
    path = "/api/system/menus/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "菜单 id")),
    request_body = MenuRequest,
    responses((status = 200, description = "更新后的菜单", body = ApiResponse<MenuAdminVo>))
)]
pub(crate) async fn update_menu(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<MenuRequest>,
) -> Result<ApiResponse<MenuAdminVo>, AppError> {
    require_perm(&user, crate::perms::MENU_EDIT)?;
    validate_menu(&req)?;

    if let Some(parent_id) = req.parent_id {
        ensure_valid_parent(&mut tx, user.tenant_id, id, parent_id).await?;
    }

    let vo = sqlx::query_as::<_, MenuAdminVo>(
        "UPDATE sys.sys_menu SET parent_id = $3, title = $4, path = $5, icon = $6, perm = $7, sort = $8, visible = $9
         WHERE id = $1 AND tenant_id = $2 RETURNING id, parent_id, title, path, icon, perm, sort, visible",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(req.parent_id)
    .bind(req.title.trim())
    .bind(req.path.trim())
    .bind(&req.icon)
    .bind(&req.perm)
    .bind(req.sort.unwrap_or(0))
    .bind(req.visible.unwrap_or(true))
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_menu_db_err)?
    .ok_or(AppError::NotFound)?;

    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.menu.update",
        "sys_menu",
        Some(&vo.id.to_string()),
        Some(serde_json::json!({ "title": vo.title, "path": vo.path })),
    )
    .await;
    Ok(ApiResponse::new(vo))
}

/// 删除菜单（有子菜单时拒绝，避免误删整棵子树）。
#[utoipa::path(
    delete,
    path = "/api/system/menus/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "菜单 id")),
    responses((status = 200, description = "删除成功"))
)]
pub(crate) async fn delete_menu(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::MENU_DELETE)?;
    let has_child: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sys.sys_menu WHERE parent_id = $1 AND tenant_id = $2)",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_one(&mut *tx)
    .await?;
    if has_child {
        return Err(AppError::Conflict("请先删除子菜单".into()));
    }
    let affected = sqlx::query("DELETE FROM sys.sys_menu WHERE id = $1 AND tenant_id = $2")
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
        "system.menu.delete",
        "sys_menu",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}

/// 唯一索引冲突映射为 409 Conflict。
fn map_menu_db_err(e: sqlx::Error) -> AppError {
    match e {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            AppError::Conflict("菜单路径已存在".into())
        }
        other => AppError::Database(other),
    }
}

/// 校验菜单字段。
fn validate_menu(req: &MenuRequest) -> Result<(), AppError> {
    let title = req.title.trim();
    if title.is_empty() || title.chars().count() > 64 {
        return Err(AppError::Validation("标题长度必须为 1~64 个字符".into()));
    }
    let path = req.path.trim();
    if !path.starts_with('/') || path.len() > 128 {
        return Err(AppError::Validation(
            "路径必须以 / 开头且不超过 128 字符".into(),
        ));
    }
    Ok(())
}
