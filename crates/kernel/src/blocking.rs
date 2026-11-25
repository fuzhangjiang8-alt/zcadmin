//! 受控的阻塞任务执行器。
//!
//! # 为什么必须限流
//!
//! Argon2id 是**内存硬**算法：单次校验按 OWASP 参数占约 19 MiB、耗 40~60ms CPU。
//! 如果直接在 tokio worker 线程里同步调用，并发 200 次登录会：
//!
//! 1. 占满所有 worker（默认 = CPU 核数），事件循环无人驱动，所有请求一起卡住；
//! 2. 峰值内存 200 × 19 MiB ≈ 3.7 GiB，容易触发 OOM。
//!
//! `spawn_blocking` 只是把工作挪到阻塞线程池，**不解决并发数**：
//! tokio 阻塞池默认上限 512，`512 × 19 MiB` 同样会打爆内存。
//! 因此这里用信号量把「同时进行的重 CPU/内存任务」限制在个位数。
//!
//! # 边界
//!
//! - 只用于**短时、可取消后无副作用**的任务（哈希、xlsx 生成、Excel 解析）；
//! - 不用于慢 I/O（数据库、HTTP），那些应该用异步客户端；
//! - 排队超时返回 429：宁可让客户端重试，也不要把整个进程拖进 swap。

use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use tokio::sync::Semaphore;

/// 全局阻塞任务并发上限（Argon2 峰值内存 = 并发数 × 19MiB）。
///
/// 默认 4：4 × 19 MiB ≈ 76 MiB，在 2C4G 的私有化环境里也安全。
/// 可用 `BLOCKING_MAX_CONCURRENCY` 覆盖，但不应超过 CPU 核数太多。
fn max_concurrency() -> usize {
    std::env::var("BLOCKING_MAX_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0 && *v <= 32)
        .unwrap_or(4)
}

/// 排队等待许可的最长时间，超时按「过载」拒绝。
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(3);

fn semaphore() -> &'static Arc<Semaphore> {
    static SEM: OnceLock<Arc<Semaphore>> = OnceLock::new();
    SEM.get_or_init(|| Arc::new(Semaphore::new(max_concurrency())))
}

/// 在阻塞线程池中执行 `f`，并限制全局并发。
///
/// # 失败语义
///
/// - 排队超过 3 秒：返回 [`BlockingError::Busy`]（调用方应映射为 429）；
/// - `spawn_blocking` 的 JoinError（任务被取消）：返回 [`BlockingError::Cancelled`]。
///
/// 副作用：任务一旦开始执行就不会被中断，调用方应在拿到结果后再写库。
pub async fn run<F, T>(f: F) -> Result<T, BlockingError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let permit = tokio::time::timeout(ACQUIRE_TIMEOUT, semaphore().acquire())
        .await
        .map_err(|_| BlockingError::Busy)?
        .map_err(|_| BlockingError::Cancelled)?;

    let result = tokio::task::spawn_blocking(move || {
        let value = f();
        // permit 在任务结束时释放：闭包捕获它，比在异步侧手动 drop 更不容易漏。
        drop(permit);
        value
    })
    .await;

    result.map_err(|_| BlockingError::Cancelled)
}

/// 阻塞任务执行失败的原因。
#[derive(Debug, thiserror::Error)]
pub enum BlockingError {
    /// 并发已达上限且排队超时（过载）。
    #[error("服务器繁忙，请稍后重试")]
    Busy,
    /// 阻塞任务被取消（通常意味着进程正在退出）。
    #[error("任务被取消")]
    Cancelled,
    /// 任务本身执行失败（保留原始上下文，便于日志排查）。
    #[error("阻塞任务执行失败: {0}")]
    Failed(#[source] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runs_on_blocking_pool() {
        let value = run(|| 1 + 1).await.expect("run");
        assert_eq!(value, 2);
    }

    #[tokio::test]
    async fn concurrency_is_bounded() {
        // 同时提交远多于上限的任务，全部应能完成（排队而非失败）。
        let mut handles = Vec::new();
        for _ in 0..32 {
            handles.push(tokio::spawn(async move {
                run(|| std::thread::sleep(Duration::from_millis(5))).await
            }));
        }
        for handle in handles {
            handle.await.expect("join").expect("run");
        }
    }
}
