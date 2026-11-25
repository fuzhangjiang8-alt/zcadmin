//! 插件内权限校验辅助与权限码常量。
//!
//! 边界：仅负责 `workflow` 插件自身的权限断言与常量暴露。

use kernel::error::AppError;
use kernel::prelude::CurrentUser;

/// 要求当前用户拥有指定权限点，否则返回 403。
pub fn require(user: &CurrentUser, code: &str) -> Result<(), AppError> {
    if user.has_permission(code) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

// 权限码常量（由 scripts/gen-perm-codes.mjs 生成，请勿手改）
/// `workflow:request:add`
pub const REQUEST_ADD: &str = "workflow:request:add";

/// `workflow:request:approve`
pub const REQUEST_APPROVE: &str = "workflow:request:approve";

/// `workflow:request:get`
pub const REQUEST_GET: &str = "workflow:request:get";

/// `workflow:request:list`
pub const REQUEST_LIST: &str = "workflow:request:list";
