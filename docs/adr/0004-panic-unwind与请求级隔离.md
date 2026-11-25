# ADR-0004：panic 策略从 abort 改为 unwind + 请求级隔离

- 状态：已采纳（2026-11-06）
- 相关代码：`Cargo.toml`（`[profile.release] panic`）、`crates/kernel/src/app.rs`（`CatchPanicLayer`）
- 相关文档：`docs/01-后端架构.md` §3.1 中间件顺序、§16.2 失败策略

## 背景

原 `[profile.release] panic = "abort"`，理由是"体积小、快、panic 本来就不该发生"。
但实际线上语义是：

1. 任意 handler 里一次越界、一次 `unwrap`、一次第三方 crate 的内部断言
   → **整个进程立即退出**；
2. 同一版本的所有副本会同时崩（同一 bug），滚动发布期间直接全站不可用；
3. 进程崩溃时所有 in-flight 请求一起失败（包括正在提交的事务）；
4. 架构文档 §3.1 / §16.2 一直写着「panic → 500，进程存活」，与实现相反。

## 决策

**改为 `panic = "unwind"`，并在中间件链加 `CatchPanicLayer`。**

- panic 被 `catch_unwind` 捕获 → 记录 ERROR 日志（含 panic 消息）→ 返回 500 + 统一的
  `problem+json`（与 `AppError::Internal` 同一格式，前端无需分支）；
- panic 消息**不回传**客户端（可能包含 SQL、路径、内存内容）；
- 代价：release 二进制略大（约 1~3%）、panic 路径略慢（正常路径无影响）。

## 备选方案

| 方案 | 不选原因 |
|---|---|
| 保持 `abort` | 单请求 bug = 全实例重启，与文档矛盾，滚动发布风险不可接受 |
| `abort` + 靠多副本 | 同一版本会同时崩；且崩溃期间的 in-flight 请求全部丢失 |
| `unwind` 但不加 CatchPanic | panic 会沿连接任务向上冒泡，hyper 会关闭连接但请求得不到响应；日志也缺少路线信息 |

## 影响

- `catch_unwind` 要求 panic payload 是 `UnwindSafe`；`CatchPanicLayer` 内部用
  `AssertUnwindSafe` 包装，语义上可接受（panic 后不再复用该请求的任何状态）。
- **必须保留 `panic = "unwind"`**：改回 `abort` 会让 `CatchPanicLayer` 静默失效，
  而且不会有编译错误——这是最容易踩的坑。
- 审计/监控：panic 应按「缺陷」处理并告警（5xx 率 + ERROR 日志），
  不能因为"被兜住了"就忽视。

## 回滚

把 `Cargo.toml` 的 `panic` 改回 `"abort"` 并移除 `CatchPanicLayer`。
回滚后必须同步修改 `docs/01` §3.1 / §16.2，明确接受「单请求 bug = 全实例重启」的风险。
