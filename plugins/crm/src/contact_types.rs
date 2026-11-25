//! 联系人接口的视图与请求类型。
//!
//! 边界：接口实现在 `contact` / `contact_read`；本模块只放 DTO。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

use kernel::response::PageQuery;

/// 联系人视图对象。
#[derive(Debug, Serialize, FromRow, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContactVo {
    /// 主键
    pub id: Uuid,
    /// customer_id（列表展示）
    pub customer_id: Uuid,
    /// name（列表展示）
    pub name: String,
    /// phone（列表展示）
    pub phone: Option<String>,
    /// email
    pub email: Option<String>,
    /// is_primary（列表展示）
    pub is_primary: bool,
    /// owner_id
    pub owner_id: Uuid,
    /// dept_id
    pub dept_id: Option<Uuid>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 更新时间
    pub updated_at: Option<DateTime<Utc>>,
}

/// 创建请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateContactRequest {
    /// customer_id
    pub customer_id: Uuid,
    /// name
    pub name: String,
    /// phone
    pub phone: Option<String>,
    /// email
    pub email: Option<String>,
    /// is_primary
    pub is_primary: bool,
}

/// 更新请求（全部可选）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateContactRequest {
    /// customer_id
    pub customer_id: Option<Uuid>,
    /// name
    pub name: Option<String>,
    /// phone
    pub phone: Option<String>,
    /// email
    pub email: Option<String>,
    /// is_primary
    pub is_primary: Option<bool>,
}

/// 列表查询参数。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContactQuery {
    /// 分页参数。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 关键字（模糊匹配可搜索字段）。
    pub keyword: Option<String>,
}
