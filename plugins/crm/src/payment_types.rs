//! 回款接口的视图与请求类型。
//!
//! 边界：接口实现在 `payment` / `payment_read`；本模块只放 DTO。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;

use kernel::response::PageQuery;

/// 回款视图对象。
#[derive(Debug, Serialize, FromRow, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaymentVo {
    /// 主键
    pub id: Uuid,
    /// contract_id（列表展示）
    pub contract_id: Uuid,
    /// payment_no（列表展示）
    pub payment_no: String,
    /// amount_cents（列表展示）
    pub amount_cents: i32,
    /// paid_at
    pub paid_at: Option<DateTime<Utc>>,
    /// method
    pub method: Option<String>,
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
pub struct CreatePaymentRequest {
    /// contract_id
    pub contract_id: Uuid,
    /// payment_no
    pub payment_no: String,
    /// amount_cents
    pub amount_cents: i32,
    /// paid_at
    pub paid_at: Option<DateTime<Utc>>,
    /// method
    pub method: Option<String>,
}

/// 更新请求（全部可选）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePaymentRequest {
    /// contract_id
    pub contract_id: Option<Uuid>,
    /// payment_no
    pub payment_no: Option<String>,
    /// amount_cents
    pub amount_cents: Option<i32>,
    /// paid_at
    pub paid_at: Option<DateTime<Utc>>,
    /// method
    pub method: Option<String>,
}

/// 列表查询参数。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaymentQuery {
    /// 分页参数。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 关键字（模糊匹配可搜索字段）。
    pub keyword: Option<String>,
    /// 回款日期起（含）。
    pub paid_from: Option<chrono::NaiveDate>,
    /// 回款日期止（含）。
    pub paid_to: Option<chrono::NaiveDate>,
}
