//! 岗位接口的视图与请求类型。
//!
//! 边界：接口实现在 `posts` / `posts_user`；本模块只放 DTO。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::response::PageQuery;

/// 岗位视图。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PostVo {
    /// 岗位 id。
    pub id: Uuid,
    /// 岗位编码。
    pub code: String,
    /// 岗位名称。
    pub name: String,
    /// 排序。
    pub sort: i32,
    /// 状态：active / disabled。
    pub status: String,
    /// 备注。
    pub remark: Option<String>,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
}

/// 列表查询。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PostQuery {
    /// 分页。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 编码或名称模糊匹配。
    pub keyword: Option<String>,
}

/// 新建/更新请求。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PostRequest {
    /// 岗位编码。
    pub code: String,
    /// 岗位名称。
    pub name: String,
    /// 排序。
    pub sort: Option<i32>,
    /// 状态。
    pub status: Option<String>,
    /// 备注。
    pub remark: Option<String>,
}
