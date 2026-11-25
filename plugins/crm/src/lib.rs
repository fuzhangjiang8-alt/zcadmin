//! CRM 插件。
//!
//! # 业务范围
//!
//! 客户主数据（customer）：编码/名称/等级/状态/负责人/联系方式。
//! 后续会扩展联系人、商机、合同、回款。
//!
//! # 为什么这个插件是「样板」
//!
//! 它完整演示了一个业务插件应有的三件事：
//! 1. 独立 schema（`crm`）与迁移；
//! 2. 声明权限点，由内核自动落库；
//! 3. 所有查询带数据权限过滤（复用登录用户的范围）。
//!
//! 新插件照抄这个结构即可（见 `plugins/demo` 的最小版）。

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod api;
mod api_customer_write;
mod api_customers;
mod api_dashboard;
mod api_metrics;
mod api_relations;
mod api_stage;
mod contact;
mod contact_read;
mod contact_types;
mod contract;
mod contract_read;
mod contract_types;
mod excel;
mod excel_import;
mod export;
mod import;
mod import_contacts;
mod import_contracts;
mod import_opportunities;
mod import_payments;
pub mod model;
mod opportunity;
mod opportunity_read;
mod opportunity_types;
mod payment;
mod payment_read;
mod payment_types;
mod perm;

use kernel::prelude::*;
use utoipa::OpenApi;

/// 本插件的迁移（编译期内嵌）。
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// 插件实现。
pub struct CrmPlugin;

impl kernel::plugin::Plugin for CrmPlugin {
    fn meta(&self) -> PluginMeta {
        PluginMeta {
            id: "crm",
            name: "客户管理",
            version: env!("CARGO_PKG_VERSION"),
            // 依赖内核身份域（用户/部门），不是插件，因此这里为空
            depends_on: &[],
            order: 200,
        }
    }

    fn migrations(&self) -> Option<&'static sqlx::migrate::Migrator> {
        Some(&MIGRATOR)
    }

    fn routes(&self) -> Router<AppState> {
        router()
    }

    /// 权限点：由内核在启动时 upsert 到 `sys_permission`。
    fn permissions(&self) -> Vec<kernel::plugin::PermissionDef> {
        perm::definitions()
    }

    /// 侧边栏菜单：内核会按租户幂等落库。
    fn menus(&self) -> Vec<kernel::plugin::MenuDef> {
        vec![
            kernel::plugin::MenuDef {
                title: "经营看板",
                path: "/crm/dashboard",
                icon: "dashboard",
                perm: Some("crm:customer:list"),
                parent: None,
                sort: 19,
            },
            kernel::plugin::MenuDef {
                title: "客户管理",
                path: "/crm/customers",
                icon: "team",
                perm: Some("crm:customer:list"),
                parent: None,
                sort: 20,
            },
            kernel::plugin::MenuDef {
                title: "联系人",
                path: "/crm/contacts",
                icon: "contacts",
                perm: Some("crm:contact:list"),
                parent: None,
                sort: 21,
            },
            kernel::plugin::MenuDef {
                title: "商机",
                path: "/crm/opportunities",
                icon: "rise",
                perm: Some("crm:opportunity:list"),
                parent: None,
                sort: 22,
            },
            kernel::plugin::MenuDef {
                title: "商机漏斗",
                path: "/crm/pipeline",
                icon: "chart",
                perm: Some("crm:opportunity:list"),
                parent: None,
                sort: 23,
            },
            kernel::plugin::MenuDef {
                title: "合同",
                path: "/crm/contracts",
                icon: "file",
                perm: Some("crm:contract:list"),
                parent: None,
                sort: 24,
            },
            kernel::plugin::MenuDef {
                title: "回款",
                path: "/crm/payments",
                icon: "money",
                perm: Some("crm:payment:list"),
                parent: None,
                sort: 25,
            },
        ]
    }

    /// 本插件的 OpenAPI 片段，由 `cargo xtask openapi` 合并进总契约。
    fn openapi(&self) -> Option<utoipa::openapi::OpenApi> {
        Some(CrmApi::openapi())
    }
}

/// 路由表：挂载到 `/api/crm`。
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/customers", get(api::list).post(api::create))
        .route(
            "/customers/{id}",
            get(api::get).patch(api::update).delete(api::delete),
        )
        .route("/opportunities/stats", get(api::opportunity_stats))
        .route(
            "/opportunities/stage-duration",
            get(api::opportunity_stage_duration),
        )
        .route(
            "/opportunities/stage-conversion",
            get(api::opportunity_stage_conversion),
        )
        .route("/opportunities/stuck", get(api::stuck_opportunities))
        .route(
            "/opportunities/{id}/contracts",
            get(api::opportunity_contracts),
        )
        .route(
            "/opportunities/{id}/stage-history",
            get(api::opportunity_stage_history),
        )
        .route("/dashboard/summary", get(api::dashboard_summary))
        .route("/dashboard/top-customers", get(api::top_customers))
        .route("/contracts/{id}/payments", get(api::contract_payments))
        .route("/customers/{id}/contracts", get(api::customer_contracts))
        .route("/customers/{id}/contacts", get(api::customer_contacts))
        .route(
            "/customers/{id}/opportunities",
            get(api::customer_opportunities),
        )
        .merge(contact::router())
        .merge(opportunity::router())
        .merge(contract::router())
        .merge(payment::router())
        .route("/customers/export", get(excel::export))
        .route("/customers/import", post(excel::import))
}

/// OpenAPI 片段。
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
        api::list, api::create, api::get, api::update, api::delete,
        contact::list, contact::detail, contact::create, contact::update, contact::delete,
        opportunity::list, opportunity::detail, opportunity::create, opportunity::update, opportunity::delete,
        contract::list, contract::detail, contract::create, contract::update, contract::delete,
        payment::list, payment::detail, payment::create, payment::update, payment::delete,
        api::customer_contacts, api::customer_opportunities, api::customer_contracts,
        api::opportunity_contracts, api::opportunity_stage_history, api::opportunity_stage_duration,
        api::opportunity_stage_conversion, api::stuck_opportunities,
        api::opportunity_stats,
        api::contract_payments, api::dashboard_summary,
        excel::export, excel::import
    ),
    components(schemas(
        model::CustomerVo,
        model::CustomerQuery,
        model::CreateCustomerRequest,
        model::UpdateCustomerRequest,
        excel::ImportResult,
        excel::ImportError,
        contact::ContactVo,
        contact::CreateContactRequest,
        contact::UpdateContactRequest,
        opportunity::OpportunityVo,
        opportunity::CreateOpportunityRequest,
        opportunity::UpdateOpportunityRequest,
        api::StageStat,
        contract::ContractVo,
        contract::CreateContractRequest,
        contract::UpdateContractRequest,
        payment::PaymentVo,
        payment::CreatePaymentRequest,
        payment::UpdatePaymentRequest,
        api::ContractPaymentsVo,
        api::DashboardSummary,
        api::MonthStat,
        api::TopCustomer,
        api::TopQuery,
        api::StageHistoryVo,
        api::StageDuration,
        api::StageConversion,
        api::StuckOpportunity,
        api::StuckQuery
    )),
    tags((name = "crm", description = "客户管理"))
)]
pub struct CrmApi;

inventory::submit! {
    kernel::plugin::PluginRegistration { create: || Box::new(CrmPlugin) }
}
