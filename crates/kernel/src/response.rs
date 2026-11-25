//! 统一响应结构。
//!
//! ## 约定
//!
//! - 成功：`{ "data": ... }`
//! - 分页：`{ "data": [...], "page": { "page": 1, "size": 20, "total": 135, "hasNext": true } }`
//! - 失败：`application/problem+json`（见 [`crate::error`]），**不使用** `{code, msg}` 双轨制。
//!
//! 这样前端只需要一个 `unwrap data` 的拦截器，HTTP 状态码表达成败，业务码只在错误时出现。

use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

/// 成功响应包装器。
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ApiResponse<T> {
    /// 实际业务数据。
    pub data: T,
}

impl<T> ApiResponse<T> {
    /// 构造一个成功响应。
    pub fn new(data: T) -> Self {
        Self { data }
    }
}

/// 让 `ApiResult<T>` 可以直接作为 axum handler 的返回值。
///
/// 约束 `T: Serialize`：只有能序列化的数据才能作为响应体，
/// 这条约束会顺着类型推导到 handler，写错类型时编译期就报错。
impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

/// 业务 handler 的标准返回类型。
///
/// 用法：
/// ```ignore
/// async fn ping() -> ApiResult<&'static str> {
///     Ok(ApiResponse::new("pong"))
/// }
/// ```
pub type ApiResult<T> = Result<ApiResponse<T>, crate::error::AppError>;

/// 分页结果。
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Page<T> {
    /// 当前页数据。
    pub data: Vec<T>,
    /// 分页元信息。
    pub page: PageMeta,
}

/// 分页元信息。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageMeta {
    /// 当前页码，从 1 开始。
    pub page: u64,
    /// 每页条数。
    pub size: u64,
    /// 总条数（大表可用估算值，见数据架构文档）。
    pub total: u64,
    /// 是否还有下一页。
    pub has_next: bool,
}

impl PageMeta {
    /// 根据页码、页大小与总数计算分页元信息。
    pub fn new(page: u64, size: u64, total: u64) -> Self {
        let has_next = page.saturating_mul(size) < total;
        Self {
            page,
            size,
            total,
            has_next,
        }
    }
}

/// 通用分页查询参数。
///
/// 服务端会强制把 `size` 限制在 `[1, 200]`，防止 `size=100000` 拖垮数据库。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PageQuery {
    /// 页码，默认 1。
    #[serde(default = "default_page", deserialize_with = "de_u64")]
    pub page: u64,
    /// 每页条数，默认 20，最大 200。
    #[serde(default = "default_size", deserialize_with = "de_u64")]
    pub size: u64,
}

/// 同时兼容 "5" 与 5：serde_html_form 传字符串，serde_urlencoded 传数字。
fn de_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct V;
    impl serde::de::Visitor<'_> for V {
        type Value = u64;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("u64 或数字字符串")
        }

        fn visit_u64<E>(self, v: u64) -> Result<u64, E> {
            Ok(v)
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<u64, E> {
            v.parse::<u64>().map_err(E::custom)
        }
    }
    deserializer.deserialize_any(V)
}

fn default_page() -> u64 {
    1
}

fn default_size() -> u64 {
    20
}

impl Default for PageQuery {
    fn default() -> Self {
        Self {
            page: default_page(),
            size: default_size(),
        }
    }
}

impl PageQuery {
    /// 归一化后的页码（至少为 1）。
    pub fn page(&self) -> u64 {
        self.page.max(1)
    }

    /// 归一化后的页大小（1..=200）。
    pub fn size(&self) -> u64 {
        self.size.clamp(1, 200)
    }

    /// SQL `OFFSET` 值；超大页码饱和到 i64 上限，避免乘法溢出或转换成负数。
    pub fn offset(&self) -> i64 {
        (self.page() - 1)
            .saturating_mul(self.size())
            .min(i64::MAX as u64) as i64
    }

    /// SQL `LIMIT` 值。
    pub fn limit(&self) -> i64 {
        self.size() as i64
    }
}

#[cfg(test)]
mod tests {
    use super::PageQuery;

    #[test]
    fn pagination_normalizes_bounds_without_overflow() {
        assert_eq!(PageQuery { page: 0, size: 0 }.offset(), 0);
        assert_eq!(PageQuery { page: 2, size: 500 }.offset(), 200);
        assert_eq!(
            PageQuery {
                page: u64::MAX,
                size: 200
            }
            .offset(),
            i64::MAX
        );
        assert_eq!(
            PageQuery {
                page: u64::MAX,
                size: 1
            }
            .offset(),
            i64::MAX
        );
    }
}
