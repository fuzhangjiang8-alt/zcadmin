//! CRM 数据模型（DTO 与数据库投影）。
//!
//! 约定：
//! - 所有对外结构都 `rename_all = "camelCase"`，与前端/OpenAPI 一致；
//! - 数据库行与对外 VO 分开，避免 `password_hash` 这类字段误暴露；
//! - 请求体独立定义，杜绝 mass assignment。

use chrono::{DateTime, Utc};
use kernel::response::PageQuery;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// 客户视图对象。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomerVo {
    /// 客户 id。
    pub id: Uuid,
    /// 客户编码（租户内唯一）。
    pub code: String,
    /// 客户名称。
    pub name: String,
    /// 客户等级（字典 customer_level）。
    pub level: String,
    /// 生命周期状态。
    pub status: String,
    /// 负责人。
    pub owner_id: Uuid,
    /// 归属部门。
    pub dept_id: Option<Uuid>,
    /// 联系电话。
    pub phone: Option<String>,
    /// 邮箱。
    pub email: Option<String>,
    /// 备注。
    pub remark: Option<String>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 更新时间。
    pub updated_at: Option<DateTime<Utc>>,
}

/// 客户列表查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CustomerQuery {
    /// 分页参数。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 关键字：编码/名称模糊匹配。
    pub keyword: Option<String>,
    /// 按等级过滤。
    pub level: Option<String>,
    /// 按状态过滤。
    pub status: Option<String>,
}

/// 创建客户请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCustomerRequest {
    /// 客户编码，租户内唯一。
    pub code: String,
    /// 客户名称。
    pub name: String,
    /// 客户等级，默认 normal。
    pub level: Option<String>,
    /// 联系电话。
    pub phone: Option<String>,
    /// 邮箱。
    pub email: Option<String>,
    /// 备注。
    pub remark: Option<String>,
}

/// 更新客户请求体（全部字段可选，传 null 表示不修改）。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCustomerRequest {
    /// 客户名称。
    pub name: Option<String>,
    /// 客户等级。
    pub level: Option<String>,
    /// 生命周期状态。
    pub status: Option<String>,
    /// 联系电话。
    pub phone: Option<String>,
    /// 邮箱。
    pub email: Option<String>,
    /// 备注。
    pub remark: Option<String>,
}

/// 允许的客户状态集合（与数据库 CHECK 约束保持一致）。
pub const ALLOWED_STATUS: &[&str] = &["potential", "following", "signed", "lost"];
