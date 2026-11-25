//! 统一错误模型。
//!
//! ## 设计目标
//!
//! 1. **业务代码只关心语义**：插件里返回 [`AppError::NotFound`] 即可，不用管 HTTP 状态码。
//! 2. **对外只暴露安全信息**：5xx 响应绝不包含 SQL、堆栈、文件路径，只给错误码。
//! 3. **格式统一**：遵循 RFC 9457（`application/problem+json`），前端可以写一份通用拦截器。
//!
//! ## 错误码规范
//!
//! ```text
//! {插件}.{领域}.{原因}     例：crm.customer.duplicated
//! ```
//!
//! 错误码一经发布不可改变语义，只能新增。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

/// 应用统一错误类型。
///
/// 插件业务代码应返回 `Result<T, AppError>`，由 axum 自动转换为 HTTP 响应。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// 参数校验失败（422）。`0` 为人类可读的说明。
    #[error("参数校验失败: {0}")]
    Validation(String),

    /// 未认证（401）：没有会话、会话过期、令牌无效。
    #[error("未认证或会话已过期")]
    Unauthorized,

    /// 已认证但无权限（403）。
    #[error("没有权限执行该操作")]
    Forbidden,

    /// 资源不存在（404）。
    ///
    /// 注意：**越权访问也应该返回 404**，避免通过状态码探测资源是否存在。
    #[error("资源不存在")]
    NotFound,

    /// 资源冲突（409），例如唯一键重复、乐观锁版本不一致。
    #[error("资源冲突: {0}")]
    Conflict(String),

    /// 触发限流（429）。
    #[error("请求过于频繁，请稍后重试")]
    RateLimited,

    /// 服务过载（503）：连接池耗尽、阻塞池排队超时等。
    ///
    /// 与 429 区分：429 是「你的请求太多」，503 是「本实例暂时没有余量」，
    /// 编排系统据此决定是否把该实例摘除。
    #[error("服务繁忙，请稍后重试")]
    Unavailable,

    /// 数据库错误（500）。对外只暴露通用文案，详情进日志。
    #[error("数据库错误")]
    Database(#[from] sqlx::Error),

    /// 其他内部错误（500）。用 `anyhow` 承载上下文，便于日志排查。
    #[error("内部错误")]
    Internal(#[from] anyhow::Error),
}

/// 业务代码的便捷别名。
pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    /// 稳定的机器可读错误码（前端按它做文案映射与分支处理）。
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Validation(_) => "sys.validation.invalid",
            AppError::Unauthorized => "sys.auth.unauthorized",
            AppError::Forbidden => "sys.auth.forbidden",
            AppError::NotFound => "sys.resource.not_found",
            AppError::Conflict(_) => "sys.resource.conflict",
            AppError::RateLimited => "sys.rate_limit.exceeded",
            AppError::Unavailable => "sys.internal.unavailable",
            AppError::Database(sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed) => {
                "sys.internal.unavailable"
            }
            AppError::Database(_) => "sys.internal.database",
            AppError::Internal(_) => "sys.internal.unexpected",
        }
    }

    /// 对应的 HTTP 状态码。
    fn status(&self) -> StatusCode {
        match self {
            AppError::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Forbidden => StatusCode::FORBIDDEN,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            AppError::Unavailable
            | AppError::Database(sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            AppError::Database(_) | AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// 对外可见的错误描述。
    ///
    /// 5xx 一律返回固定文案：真实原因只写日志，防止泄漏内部结构。
    fn public_message(&self) -> String {
        match self {
            AppError::Validation(m) | AppError::Conflict(m) => m.clone(),
            // 获取连接失败表示服务暂不可用，不能误导客户端为不可重试的业务失败。
            AppError::Database(sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed) => {
                AppError::Unavailable.to_string()
            }
            AppError::Unauthorized
            | AppError::Forbidden
            | AppError::NotFound
            | AppError::RateLimited
            | AppError::Unavailable
            | AppError::Database(_)
            | AppError::Internal(_) => self.to_string(),
        }
    }
}

impl From<crate::blocking::BlockingError> for AppError {
    fn from(value: crate::blocking::BlockingError) -> Self {
        match value {
            // 排队超时是「本实例没有余量」，与「客户端请求太频繁」语义不同。
            crate::blocking::BlockingError::Busy => AppError::Unavailable,
            // 任务被取消意味着进程正在退出，请求必然失败。
            crate::blocking::BlockingError::Cancelled => {
                AppError::Internal(anyhow::anyhow!("阻塞任务被取消（进程可能正在退出）"))
            }
            crate::blocking::BlockingError::Failed(error) => AppError::Internal(error),
        }
    }
}

/// RFC 9457 问题详情响应体。
#[derive(Debug, Serialize)]
struct ProblemDetails {
    /// 问题类型的 URI（这里用错误码替代，简化部署）。
    #[serde(rename = "type")]
    type_uri: String,
    /// 简短标题。
    title: String,
    /// HTTP 状态码。
    status: u16,
    /// 机器可读错误码。
    code: &'static str,
    /// 人类可读详情。
    detail: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // 5xx 打完整日志（含 backtrace 与 source 链），4xx 只打 warn。
        // 这样线上排障看日志，用户看响应，两边都不吃亏。
        let status = self.status();
        if status.is_server_error() {
            tracing::error!(error = ?self, code = self.code(), "请求处理失败（内部错误）");
        } else {
            tracing::warn!(code = self.code(), "请求被拒绝: {}", self.public_message());
        }

        let body = ProblemDetails {
            type_uri: format!("https://errors.zcadmin.local/{}", self.code()),
            title: status.canonical_reason().unwrap_or("Unknown").to_string(),
            status: status.as_u16(),
            code: self.code(),
            detail: self.public_message(),
        };

        let mut response = (status, Json(body)).into_response();
        // Json 默认是 application/json；错误契约要求明确的 problem 媒体类型。
        response.headers_mut().insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/problem+json"),
        );
        // 告诉客户端/网关多久后可以重试：没有这个头，客户端通常会立刻重试，
        // 反而把过载放大成雪崩。
        if matches!(
            status,
            StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
        ) {
            response.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from_static("5"),
            );
        }
        response
    }
}
