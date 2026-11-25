//! 商机接口的视图与请求类型。
//!
//! 边界：接口实现在 `opportunity` / `opportunity_read`；本模块只放 DTO。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

use kernel::response::PageQuery;

/// 商机视图对象。
#[derive(Debug, Serialize, FromRow, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpportunityVo {
    /// 主键
    pub id: Uuid,
    /// customer_id（列表展示）
    pub customer_id: Uuid,
    /// name（列表展示）
    pub name: String,
    /// amount_cents（列表展示）
    pub amount_cents: Option<i32>,
    /// stage（列表展示）
    pub stage: String,
    /// expected_close
    pub expected_close: Option<DateTime<Utc>>,
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
pub struct CreateOpportunityRequest {
    /// customer_id
    pub customer_id: Uuid,
    /// name
    pub name: String,
    /// amount_cents
    pub amount_cents: Option<i32>,
    /// stage
    pub stage: String,
    /// expected_close
    pub expected_close: Option<DateTime<Utc>>,
}

/// 更新请求（全部可选）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOpportunityRequest {
    /// customer_id
    pub customer_id: Option<Uuid>,
    /// name
    pub name: Option<String>,
    /// amount_cents
    pub amount_cents: Option<i32>,
    /// stage
    pub stage: Option<String>,
    /// expected_close
    pub expected_close: Option<DateTime<Utc>>,
}

/// 列表查询参数。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpportunityQuery {
    /// 分页参数。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 关键字（模糊匹配可搜索字段）。
    pub keyword: Option<String>,
}
