//! 企业级轻量审批流插件（`workflow`）。
//!
//! # 插件定位与可插拔边界
//!
//! - 面向需要流程审批（合同审批、折扣特批、退款审批、通用请示）的企业；
//! - 某些内部系统不需要审批模块时，支持**双层可插拔**：
//!   1. 编译期：`cargo build -p server --no-default-features --features plugin-crm` 彻底裁剪；
//!   2. 运行期：`DISABLED_PLUGINS=workflow` 或 `config/default.toml` 配置 `disabled = ["workflow"]` 跳过加载。
//! - 独立 schema（`workflow`）、RLS `ENABLE + FORCE`、权限点与菜单自声明。

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod api;
pub mod model;
pub mod perm;

use axum::routing::{get, post};
use kernel::prelude::*;
use model::{CreateWorkflowRequest, DecideWorkflowRequest, WorkflowRequestVo, WorkflowStatsVo};
use utoipa::OpenApi;

/// 本插件的迁移（编译期内嵌）。
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// 审批流插件实现。
pub struct WorkflowPlugin;

impl kernel::plugin::Plugin for WorkflowPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta {
            id: "workflow",
            name: "审批中心",
            version: env!("CARGO_PKG_VERSION"),
            depends_on: &[],
            order: 250,
        }
    }

    fn migrations(&self) -> Option<&'static sqlx::migrate::Migrator> {
        Some(&MIGRATOR)
    }

    fn routes(&self) -> Router<AppState> {
        router()
    }

    fn permissions(&self) -> Vec<kernel::plugin::PermissionDef> {
        vec![
            kernel::plugin::PermissionDef {
                code: "workflow:request:list",
                name: "审批单查询",
            },
            kernel::plugin::PermissionDef {
                code: "workflow:request:get",
                name: "审批单详情",
            },
            kernel::plugin::PermissionDef {
                code: "workflow:request:add",
                name: "发起审批",
            },
            kernel::plugin::PermissionDef {
                code: "workflow:request:approve",
                name: "审批处理",
            },
        ]
    }

    fn menus(&self) -> Vec<kernel::plugin::MenuDef> {
        vec![kernel::plugin::MenuDef {
            title: "审批中心",
            path: "/workflow/requests",
            icon: "audit",
            perm: Some(perm::REQUEST_LIST),
            parent: None,
            sort: 35,
        }]
    }

    fn openapi(&self) -> Option<utoipa::openapi::OpenApi> {
        Some(WorkflowApi::openapi())
    }
}

/// 构造审批流插件路由（挂载在 `/api/workflow` 下）。
pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/requests",
            get(api::list_requests).post(api::create_request),
        )
        .route("/requests/stats", get(api::request_stats))
        .route("/requests/{id}", get(api::get_request))
        .route("/requests/{id}/decide", post(api::decide_request))
}

/// 审批流插件 OpenAPI 片段。
#[derive(OpenApi)]
#[openapi(
    paths(
        api::list_requests,
        api::request_stats,
        api::get_request,
        api::create_request,
        api::decide_request
    ),
    components(schemas(
        WorkflowRequestVo,
        WorkflowStatsVo,
        CreateWorkflowRequest,
        DecideWorkflowRequest
    )),
    tags((name = "workflow", description = "企业轻量审批流（可插拔）"))
)]
pub struct WorkflowApi;

inventory::submit! {
    kernel::plugin::PluginRegistration { create: || Box::new(WorkflowPlugin) }
}
