//! 部门（组织树）接口：树查询与 CRUD。
//!
//! 边界：只处理 `/depts`；菜单在 `system_menus`，字典在 `system_dicts`。
//! 关键约束：部门名租户内可重复，但父部门必须属于同一租户。

use axum::extract::{Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::system::{ensure_dept_in_tenant, require_perm};
use crate::tenant::TenantTx;

/// 部门路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/depts", get(list_depts).post(create_dept))
        .route("/depts/{id}", put(update_dept).delete(delete_dept))
}

#[derive(Debug, Clone, FromRow)]
struct DeptRow {
    id: Uuid,
    parent_id: Option<Uuid>,
    name: String,
    sort: i32,
}

/// 部门树节点。
#[derive(Debug, Serialize, Clone, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeptNode {
    /// 部门 id。
    pub id: Uuid,
    /// 部门名称。
    pub name: String,
    /// 排序值。
    pub sort: i32,
    /// 子部门。
    #[schema(no_recursion)]
    pub children: Vec<DeptNode>,
}

/// 部门树。
#[utoipa::path(
    get,
    path = "/api/system/depts",
    tag = "system",
    responses((status = 200, description = "部门树", body = ApiResponse<Vec<DeptNode>>))
)]
async fn list_depts(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<Vec<DeptNode>>, AppError> {
    require_perm(&user, crate::perms::USER_LIST)?;
    let rows = sqlx::query_as::<_, DeptRow>(
        "SELECT id, parent_id, name, sort FROM sys.sys_dept WHERE tenant_id = $1 AND deleted_at IS NULL ORDER BY sort, name, id",
    )
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(ApiResponse::new(build_dept_tree(rows)))
}

/// 将平铺部门行组装为树（必须先收集全部父子映射再构造根节点，防止父节点排在子节点前导致子树丢失）。
fn build_dept_tree(rows: Vec<DeptRow>) -> Vec<DeptNode> {
    let ids: HashSet<Uuid> = rows.iter().map(|d| d.id).collect();
    let mut children: HashMap<Uuid, Vec<DeptRow>> = HashMap::new();
    let mut root_rows = Vec::new();
    for row in rows {
        match row.parent_id {
            Some(pid) if ids.contains(&pid) => children.entry(pid).or_default().push(row),
            _ => root_rows.push(row),
        }
    }
    root_rows
        .into_iter()
        .map(|row| to_dept_node(row, &mut children))
        .collect()
}

fn to_dept_node(row: DeptRow, children: &mut HashMap<Uuid, Vec<DeptRow>>) -> DeptNode {
    let mut node = leaf_node(row.id, &row.name, row.sort);
    if let Some(kids) = children.remove(&row.id) {
        node.children = kids
            .into_iter()
            .map(|c| to_dept_node(c, children))
            .collect();
    }
    node
}

fn leaf_node(id: Uuid, name: &str, sort: i32) -> DeptNode {
    DeptNode {
        id,
        name: name.to_string(),
        sort,
        children: Vec::new(),
    }
}

fn validate_dept_name(raw: &str) -> Result<&str, AppError> {
    let name = raw.trim();
    (!name.is_empty() && name.chars().count() <= 64)
        .then_some(name)
        .ok_or_else(|| AppError::Validation("部门名称长度必须为 1~64 个字符".into()))
}

/// 创建部门请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateDeptRequest {
    /// 部门名称。
    pub name: String,
    /// 上级部门 id（空表示根部门）。
    pub parent_id: Option<Uuid>,
    /// 排序值（默认 0）。
    pub sort: Option<i32>,
}

/// 新建部门。
#[utoipa::path(
    post,
    path = "/api/system/depts",
    tag = "system",
    request_body = CreateDeptRequest,
    responses((status = 200, description = "创建成功", body = ApiResponse<DeptNode>))
)]
async fn create_dept(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<CreateDeptRequest>,
) -> Result<ApiResponse<DeptNode>, AppError> {
    require_perm(&user, crate::perms::DEPT_ADD)?;
    let name = validate_dept_name(&req.name)?;
    if let Some(parent_id) = req.parent_id {
        ensure_dept_in_tenant(&mut tx, user.tenant_id, parent_id).await?;
    }
    let id = Uuid::now_v7();
    let sort = req.sort.unwrap_or(0);
    sqlx::query("INSERT INTO sys.sys_dept (id, tenant_id, parent_id, name, sort) VALUES ($1, $2, $3, $4, $5)")
        .bind(id)
        .bind(user.tenant_id)
        .bind(req.parent_id)
        .bind(name)
        .bind(sort)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.dept.create",
        "sys_dept",
        None,
        None,
    )
    .await;
    Ok(ApiResponse::new(leaf_node(id, name, sort)))
}

/// 更新部门请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDeptRequest {
    /// 部门名称。
    pub name: String,
    /// 排序值。
    pub sort: Option<i32>,
}

/// 更新部门（改名 / 排序）。
#[utoipa::path(
    put,
    path = "/api/system/depts/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "部门 id")),
    request_body = UpdateDeptRequest,
    responses((status = 200, description = "更新后的部门", body = ApiResponse<DeptNode>))
)]
async fn update_dept(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateDeptRequest>,
) -> Result<ApiResponse<DeptNode>, AppError> {
    require_perm(&user, crate::perms::DEPT_EDIT)?;
    let name = validate_dept_name(&req.name)?;
    let sort: Option<i32> = sqlx::query_scalar(
        "UPDATE sys.sys_dept SET name = $3, sort = COALESCE($4, sort)
         WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL RETURNING sort",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(name)
    .bind(req.sort)
    .fetch_optional(&mut *tx)
    .await?;
    let sort = sort.ok_or(AppError::NotFound)?;
    tx.commit().await?;
    crate::audit::record_user(
        &state.db,
        &user,
        "system.dept.update",
        "sys_dept",
        None,
        None,
    )
    .await;
    Ok(ApiResponse::new(leaf_node(id, name, sort)))
}

/// 删除部门（有子部门或用户时拒绝，软删）。
#[utoipa::path(
    delete,
    path = "/api/system/depts/{id}",
    tag = "system",
    params(("id" = Uuid, Path, description = "部门 id")),
    responses((status = 200, description = "删除成功"))
)]
async fn delete_dept(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> Result<ApiResponse<()>, AppError> {
    require_perm(&user, crate::perms::DEPT_DELETE)?;
    let (has_child, has_user): (bool, bool) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM sys.sys_dept WHERE parent_id = $1 AND tenant_id = $2 AND deleted_at IS NULL),
                EXISTS(SELECT 1 FROM sys.sys_user WHERE dept_id = $1 AND tenant_id = $2 AND deleted_at IS NULL)",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_one(&mut *tx)
    .await?;
    if has_child {
        return Err(AppError::Conflict("请先删除子部门".into()));
    }
    if has_user {
        return Err(AppError::Conflict("该部门下还有用户，不能删除".into()));
    }
    let affected = sqlx::query(
        "UPDATE sys.sys_dept SET deleted_at = now() WHERE id = $1 AND tenant_id = $2 AND deleted_at IS NULL",
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
        "system.dept.delete",
        "sys_dept",
        None,
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_tree_when_parent_precedes_children() {
        let root_id = Uuid::now_v7();
        let rows = vec![
            (root_id, None, "总部", 1),
            (Uuid::now_v7(), Some(root_id), "研发部", 2),
        ]
        .into_iter()
        .map(|(id, parent_id, name, sort)| DeptRow {
            id,
            parent_id,
            name: name.into(),
            sort,
        })
        .collect();
        let tree = build_dept_tree(rows);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].children[0].name, "研发部");
    }
}
