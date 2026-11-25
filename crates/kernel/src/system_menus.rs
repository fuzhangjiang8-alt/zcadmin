//! 菜单接口：当前用户菜单树、管理员菜单 CRUD、插件菜单同步。
//!
//! 边界：只处理 `/menus`；部门在 `system_depts`，字典在 `system_dicts`。
//! 关键约束：父菜单必须同租户；有子菜单时禁止删除。

use axum::extract::State;
use axum::routing::get;
use axum::Router;
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::response::ApiResponse;
use crate::state::AppState;
use crate::tenant::TenantTx;

/// 菜单路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/menus", get(list_menus))
        .merge(crate::system_menus_admin::router())
}

/// 菜单行（数据库投影）。
#[derive(Debug, Clone, FromRow)]
struct MenuRow {
    id: Uuid,
    parent_id: Option<Uuid>,
    title: String,
    path: String,
    icon: Option<String>,
    perm: Option<String>,
}

/// 菜单树节点（下发前端）。
#[derive(Debug, Serialize, Clone, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MenuNode {
    /// 菜单 id。
    pub id: Uuid,
    /// 标题。
    pub title: String,
    /// 前端路由路径。
    pub path: String,
    /// 图标名（前端图标映射表的 key）。
    pub icon: Option<String>,
    /// 子菜单。
    #[schema(no_recursion)]
    pub children: Vec<MenuNode>,
}

/// 获取当前用户可见的菜单树。
///
/// 过滤规则：
/// 1. 超级管理员（`*`）看到全部；
/// 2. 普通用户只看到 `perm` 为空或自己拥有的菜单；
/// 3. 父菜单被过滤时，子菜单不会"漂浮"到根节点（整棵子树一起隐藏）。
#[utoipa::path(
    get,
    path = "/api/system/menus",
    tag = "system",
    responses((status = 200, description = "当前用户可见的菜单树", body = ApiResponse<Vec<MenuNode>>))
)]
async fn list_menus(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> Result<ApiResponse<Vec<MenuNode>>, AppError> {
    let rows = sqlx::query_as::<_, MenuRow>(
        r#"
        SELECT id, parent_id, title, path, icon, perm
        FROM sys.sys_menu
        WHERE tenant_id = $1 AND visible = true
        ORDER BY sort, id
        "#,
    )
    .bind(user.tenant_id)
    .fetch_all(&mut *tx)
    .await?;

    // 第一步：权限过滤
    let allowed: Vec<MenuRow> = rows
        .into_iter()
        .filter(|m| match &m.perm {
            None => true,
            Some(p) => user.has_permission(p),
        })
        .collect();

    // 第二步：组树（父节点被过滤掉时，子节点一并丢弃，避免孤儿菜单）
    //
    // 必须先收集完所有父子关系再生成根节点：菜单行按 sort 排序，
    // 子节点可能排在父节点之后，边遍历边 to_node 会拿到空 children。
    let mut children_map: std::collections::HashMap<Uuid, Vec<MenuRow>> =
        std::collections::HashMap::new();
    let ids: std::collections::HashSet<Uuid> = allowed.iter().map(|m| m.id).collect();

    for row in &allowed {
        if let Some(pid) = row.parent_id {
            if ids.contains(&pid) {
                children_map.entry(pid).or_default().push(row.clone());
            }
        }
    }

    let mut roots: Vec<MenuNode> = Vec::new();
    for row in allowed {
        if row.parent_id.is_none() {
            roots.push(to_node(row, &mut children_map));
        }
    }

    tx.commit().await?;
    Ok(ApiResponse::new(roots))
}

/// 校验父菜单属于当前租户。
pub(crate) async fn ensure_menu_in_tenant(
    conn: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    menu_id: Uuid,
) -> Result<(), AppError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sys.sys_menu WHERE id = $1 AND tenant_id = $2)",
    )
    .bind(menu_id)
    .bind(tenant_id)
    .fetch_one(&mut *conn)
    .await?;
    exists.then_some(()).ok_or(AppError::NotFound)
}

/// 校验更新时的父菜单合法（同租户且不形成环）。
pub(crate) async fn ensure_valid_parent(
    conn: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    menu_id: Uuid,
    parent_id: Uuid,
) -> Result<(), AppError> {
    if parent_id == menu_id {
        return Err(AppError::Validation("父菜单不能是自己".into()));
    }
    ensure_menu_in_tenant(conn, tenant_id, parent_id).await?;
    let is_descendant: bool = sqlx::query_scalar(
        "WITH RECURSIVE ancestors AS (
             SELECT id, parent_id FROM sys.sys_menu WHERE id = $1 AND tenant_id = $2
             UNION ALL
             SELECT m.id, m.parent_id FROM sys.sys_menu m JOIN ancestors a ON m.id = a.parent_id
         )
         SELECT EXISTS (SELECT 1 FROM ancestors WHERE id = $3)",
    )
    .bind(parent_id)
    .bind(tenant_id)
    .bind(menu_id)
    .fetch_one(&mut *conn)
    .await?;
    if is_descendant {
        return Err(AppError::Validation(
            "不能把菜单移动到自己的子菜单下".into(),
        ));
    }
    Ok(())
}

/// 递归把一行转成节点。
fn to_node(
    row: MenuRow,
    children_map: &mut std::collections::HashMap<Uuid, Vec<MenuRow>>,
) -> MenuNode {
    let children = children_map
        .remove(&row.id)
        .unwrap_or_default()
        .into_iter()
        .map(|child| to_node(child, children_map))
        .collect();

    MenuNode {
        id: row.id,
        title: row.title,
        path: row.path,
        icon: row.icon,
        children,
    }
}

// =============================================================================
// 数据字典
// =============================================================================
