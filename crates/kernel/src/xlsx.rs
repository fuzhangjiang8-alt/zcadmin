//! xlsx 生成的统一执行入口。
//!
//! # 为什么需要这一层
//!
//! `rust_xlsxwriter::Workbook::save_to_buffer()` 是纯同步的 CPU/内存密集操作：
//! 压缩、XML 拼接、字符串表都在调用线程里完成。直接写在 async handler 或
//! `tokio::spawn` 的后台任务里会占住 tokio worker，导出并发一高就拖慢整个实例。
//! AGENTS 铁律 16 要求这类操作统一走 `kernel::blocking::run`（受控阻塞池，
//! 默认并发 4，排队超时返回 503），本模块把「构造 Workbook → 保存」收进一个闭包，
//! 让所有调用点只需要改一行，避免每个站点各自发明写法。
//!
//! # 边界
//!
//! - 只负责**生成**（Workbook 构造与 `save_to_buffer`）；解析走
//!   `user_import_parse`，文件落盘走 `storage`。
//! - 闭包必须是 `FnOnce() -> anyhow::Result<T> + Send + 'static`：不要在闭包里
//!   持有 `&PgConnection`、事务或任何非 `'static` 借用；数据行应先查询成
//!   拥有的 `Vec` 再移入闭包。
//! - `Workbook` 内含 `Arc<Mutex<...>>`，是 `Send` 而非 `Sync`；`spawn_blocking`
//!   只要求 `Send`，因此可在闭包内构造。
//!
//! # 失败语义
//!
//! - 排队超过 3 秒 / 池已关闭 → `AppError::Unavailable`（503 + `Retry-After`）；
//! - 闭包自身返回 `Err` → `AppError::Internal`，保留原始上下文供日志排查；
//! - 生成失败**不写库**：调用方应在拿到 bytes 之后再提交事务。

use crate::blocking::BlockingError;
use crate::error::AppError;

/// 在受控阻塞池中生成 xlsx，返回闭包的结果。
///
/// # 失败语义
///
/// - 过载/取消：映射为 [`AppError::Unavailable`] 或 `Internal`；
/// - 闭包 `Err`：包装为 [`AppError::Internal`]，不丢失业务上下文。
///
/// # 副作用
///
/// 占用全局阻塞池许可直到闭包返回；任务一旦开始执行就不会被中断。
pub async fn run<F, T>(f: F) -> Result<T, AppError>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    match crate::blocking::run(f).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(AppError::Internal(error)),
        Err(BlockingError::Failed(error)) => Err(AppError::Internal(error)),
        Err(other) => Err(other.into()),
    }
}

/// `anyhow::Result` 版本的入口，供后台任务（`exports_runner`）使用。
///
/// 后台任务没有 HTTP 响应，但仍需把「过载」与「生成失败」区分开：
/// 前者可重试，后者应把任务标记为 failed 并记录原因。
///
/// # 失败语义
///
/// 排队超时/取消/闭包失败都返回带上下文的 `anyhow::Error`，由调用方记入任务状态。
pub async fn run_anyhow<F, T>(f: F) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let result = crate::blocking::run(f)
        .await
        .map_err(|e| anyhow::anyhow!("xlsx 阻塞池调度失败: {e}"))?;
    result
}
