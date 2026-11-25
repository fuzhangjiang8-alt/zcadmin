//! 组织与配置路由聚合（菜单 / 部门 / 字典）。
//!
//! # 职责
//!
//! 只把 `system_menus`、`system_depts`、`system_dicts` 的子路由合并，
//! 并汇总它们的 OpenAPI 片段；不承载具体 handler。
//!
//! # 边界
//!
//! 用户/角色/权限在 `system_identity`；租户在 `tenant_admin`。
//! 本模块只处理「组织与配置」数据，所有写接口要求 `system:menu|dict|dept:*`。
//!
//! # 关键约束
//!
//! 从 `system.rs` 拆出。`sync_plugin_menus` 被 `app.rs` 调用，
//! 由 `system` 重新导出以保持入口不变。

use crate::state::AppState;
use axum::Router;

/// 菜单/字典/部门路由（挂在 `/api/system` 下）。
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .merge(crate::system_menus::router())
        .merge(crate::system_depts::router())
        .merge(crate::system_dicts::router())
}

/// 菜单/字典/部门接口的 OpenAPI 片段。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        crate::system_menus::list_menus,
        crate::system_menus_admin::admin_list_menus,
        crate::system_menus_admin::create_menu,
        crate::system_menus_admin::update_menu,
        crate::system_menus_admin::delete_menu,
        crate::system_depts::list_depts,
        crate::system_depts::create_dept,
        crate::system_depts::update_dept,
        crate::system_depts::delete_dept,
        crate::system_dicts::list_dict_types,
        crate::system_dicts::create_dict_type,
        crate::system_dicts_edit::update_dict_type,
        crate::system_dicts_edit::delete_dict_type,
        crate::system_dicts::list_dict_items,
        crate::system_dicts::create_dict_item,
        crate::system_dicts_edit::update_dict_item,
        crate::system_dicts_edit::delete_dict_item
    ),
    components(schemas(
        crate::system_menus::MenuNode,
        crate::system_menus_admin::MenuAdminVo,
        crate::system_menus_admin::MenuRequest,
        crate::system_depts::DeptNode,
        crate::system_depts::CreateDeptRequest,
        crate::system_depts::UpdateDeptRequest,
        crate::system_dicts::DictTypeVo,
        crate::system_dicts::DictItemVo,
        crate::system_dicts::CreateDictTypeRequest,
        crate::system_dicts::CreateDictItemRequest,
        crate::system_dicts_edit::UpdateDictTypeRequest,
        crate::system_dicts_edit::UpdateDictItemRequest
    ))
)]
pub struct OrgApi;
