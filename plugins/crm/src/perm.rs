//! 插件内权限校验辅助。

use kernel::error::AppError;
use kernel::prelude::CurrentUser;

/// 要求当前用户拥有指定权限。
pub fn require(user: &CurrentUser, code: &str) -> Result<(), AppError> {
    if user.has_permission(code) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// 返回 CRM 插件声明的全部权限点。
pub fn definitions() -> Vec<kernel::plugin::PermissionDef> {
    use kernel::plugin::PermissionDef;
    vec![
        PermissionDef {
            code: "crm:customer:list",
            name: "客户查询",
        },
        PermissionDef {
            code: "crm:customer:get",
            name: "客户详情",
        },
        PermissionDef {
            code: "crm:customer:add",
            name: "客户新增",
        },
        PermissionDef {
            code: "crm:customer:edit",
            name: "客户编辑",
        },
        PermissionDef {
            code: "crm:customer:delete",
            name: "客户删除",
        },
        PermissionDef {
            code: "crm:customer:export",
            name: "客户导出",
        },
        PermissionDef {
            code: "crm:customer:import",
            name: "客户导入",
        },
        PermissionDef {
            code: "crm:contact:list",
            name: "联系人查询",
        },
        PermissionDef {
            code: "crm:contact:add",
            name: "联系人新增",
        },
        PermissionDef {
            code: "crm:contact:edit",
            name: "联系人编辑",
        },
        PermissionDef {
            code: "crm:contact:delete",
            name: "联系人删除",
        },
        PermissionDef {
            code: "crm:opportunity:list",
            name: "商机查询",
        },
        PermissionDef {
            code: "crm:opportunity:add",
            name: "商机新增",
        },
        PermissionDef {
            code: "crm:opportunity:edit",
            name: "商机编辑",
        },
        PermissionDef {
            code: "crm:opportunity:delete",
            name: "商机删除",
        },
        PermissionDef {
            code: "crm:contract:list",
            name: "合同查询",
        },
        PermissionDef {
            code: "crm:contract:add",
            name: "合同新增",
        },
        PermissionDef {
            code: "crm:contract:edit",
            name: "合同编辑",
        },
        PermissionDef {
            code: "crm:contract:delete",
            name: "合同删除",
        },
        PermissionDef {
            code: "crm:payment:list",
            name: "回款查询",
        },
        PermissionDef {
            code: "crm:payment:add",
            name: "回款新增",
        },
        PermissionDef {
            code: "crm:payment:edit",
            name: "回款编辑",
        },
        PermissionDef {
            code: "crm:payment:delete",
            name: "回款删除",
        },
    ]
}

// 权限码常量（由 scripts/gen-perm-codes.mjs 生成，请勿手改）
/// `crm:contact:add`
pub const CONTACT_ADD: &str = "crm:contact:add";

/// `crm:contact:delete`
pub const CONTACT_DELETE: &str = "crm:contact:delete";

/// `crm:contact:edit`
pub const CONTACT_EDIT: &str = "crm:contact:edit";

/// `crm:contact:list`
pub const CONTACT_LIST: &str = "crm:contact:list";

/// `crm:contract:add`
pub const CONTRACT_ADD: &str = "crm:contract:add";

/// `crm:contract:delete`
pub const CONTRACT_DELETE: &str = "crm:contract:delete";

/// `crm:contract:edit`
pub const CONTRACT_EDIT: &str = "crm:contract:edit";

/// `crm:contract:list`
pub const CONTRACT_LIST: &str = "crm:contract:list";

/// `crm:customer:add`
pub const CUSTOMER_ADD: &str = "crm:customer:add";

/// `crm:customer:delete`
pub const CUSTOMER_DELETE: &str = "crm:customer:delete";

/// `crm:customer:edit`
pub const CUSTOMER_EDIT: &str = "crm:customer:edit";

/// `crm:customer:export`
pub const CUSTOMER_EXPORT: &str = "crm:customer:export";

/// `crm:customer:get`
pub const CUSTOMER_GET: &str = "crm:customer:get";

/// `crm:customer:import`
pub const CUSTOMER_IMPORT: &str = "crm:customer:import";

/// `crm:customer:list`
pub const CUSTOMER_LIST: &str = "crm:customer:list";

/// `crm:opportunity:add`
pub const OPPORTUNITY_ADD: &str = "crm:opportunity:add";

/// `crm:opportunity:delete`
pub const OPPORTUNITY_DELETE: &str = "crm:opportunity:delete";

/// `crm:opportunity:edit`
pub const OPPORTUNITY_EDIT: &str = "crm:opportunity:edit";

/// `crm:opportunity:list`
pub const OPPORTUNITY_LIST: &str = "crm:opportunity:list";

/// `crm:payment:add`
pub const PAYMENT_ADD: &str = "crm:payment:add";

/// `crm:payment:delete`
pub const PAYMENT_DELETE: &str = "crm:payment:delete";

/// `crm:payment:edit`
pub const PAYMENT_EDIT: &str = "crm:payment:edit";

/// `crm:payment:list`
pub const PAYMENT_LIST: &str = "crm:payment:list";
