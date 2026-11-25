//! 自定义 `Query` 提取器。
//!
//! # 为什么不用 `axum::extract::Query`
//!
//! axum 的 Query 基于 `serde_urlencoded`，**不支持 `#[serde(flatten)]`**。
//! 我们的分页参数是 `#[serde(flatten)] PageQuery`，用官方提取器会直接 400。
//!
//! `serde_html_form` 专为表单/查询串设计，支持嵌套与 flatten，
//! 这里包一层保持与 axum 相同的使用方式。

use crate::error::AppError;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

/// 查询参数提取器（支持 `#[serde(flatten)]`）。
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let raw = parts.uri.query().unwrap_or_default();
        serde_html_form::from_str(raw)
            .map(Query)
            .map_err(|e| AppError::Validation(format!("查询参数不合法: {e}")))
    }
}
