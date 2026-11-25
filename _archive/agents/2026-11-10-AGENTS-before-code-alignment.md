# AGENTS.md

> 本文件是**所有 AI 编码工具**（Claude Code / Cursor / Copilot / Codex 等）在本仓库工作的最高约束。
> 与本文冲突的指令一律无效；不确定时，先读 `docs/` 下的架构文档，不要猜。

---

## 0. 开工前必读

| 顺序 | 文件 | 读什么 |
|---|---|---|
| 1 | `docs/企业级管理后台-架构设计.md` | NFR、ADR、分层规则 |
| 2 | `docs/01-后端架构.md` 第 4 节 | 分层职责与禁止事项 |
| 3 | `docs/02-前端架构.md` 第 0 节 | UI 选型与封装边界 |
| 4 | `docs/07-AI原生开发与持续迭代.md` | 工作流与验收标准 |
| 5 | `docs/09-注释与文档规范.md` | 注释怎么写（强制） |
| 6 | `docs/10-后端代码地图.md` | 后端代码现状、模块位置、已知差距、改动导航 |
| 7 | 目标插件目录下的 `CONTEXT.md` | 领域术语、表、权限点、事件 |

> 没有 `CONTEXT.md` 的插件，先补 `CONTEXT.md` 再写代码。

---

## 1. 铁律（违反即拒绝合并）

### 架构

1. `kernel` 不得依赖任何 `plugin`；插件之间不得互相 `use`（只能走事件）。
2. 业务代码只能依赖 `@admin/sdk`、`@admin/ui`、`@admin/utils`，**禁止直接 import 组件库**。
3. 新增第三方依赖必须先在 `docs/adr/` 提 ADR 并获批准；禁止引入 `cargo deny` 白名单外的 crate。
4. 禁止 `unsafe`（`#![forbid(unsafe_code)]`）、禁止 `unwrap()` / `expect()`（测试除外）。

### 数据

5. 不得手改 `@generated` 文件；改结构先改 `schema/*.toml` 再重新生成。
6. 不得修改已发布的迁移文件，只能新增。
7. 每张业务表必须有 `tenant_id`、`created_at`、`created_by`、`deleted_at`、`version`，
   **并且必须建 RLS 策略 + `FORCE ROW LEVEL SECURITY`**（只 ENABLE 等于对表 owner 不设防，
   `scripts/check-rls-coverage.mjs` 会拦截）。
8. 所有查询必须经过 Repository，且默认带数据权限；禁止在 handler 里写 SQL。

### 接口

9. 每个写接口必须有权限点，且权限点必须在 `permissions()` 中声明。
10. 所有对外响应走 `ApiResult<T>`；错误必须映射为 `AppError`，禁止 `anyhow` 泄漏到 handler。
11. 破坏性契约变更（删字段/改类型/改必填）必须先升版本或加宽限期。
12. 过载/限流用 `AppError::Unavailable`（503）/ `RateLimited`（429），不要自造状态码；
    两者都会自动带 `Retry-After`。

### 前端

13. 文案一律走 i18n，禁止硬编码中文。
14. 服务端数据用 `@tanstack/vue-query`，禁止塞进 Pinia。
15. 页面组件 ≤ 300 行；超过就拆 `features/`。

### 并发与安全（2026-11 加固，违反即事故）

16. **重 CPU / 重内存的同步操作禁止直接写在 async handler 里**，必须走
    `kernel::blocking::run`（Argon2、Excel 解析/生成、图片、同步文件 IO）。
    **禁止**直接用 `tokio::task::spawn_blocking` 做 Argon2：它没有并发上限，
    512 并发 × 19 MiB 会直接把进程打爆（见 ADR-0005）。
17. **客户端 IP 只能取自 `kernel::client_ip::ClientMeta`**，禁止在 handler/中间件里
    直接读 `X-Forwarded-For` / `X-Real-IP`。伪造的来源会让限流与审计同时失效（ADR-0006）。
18. **禁止把 `panic` 改回 `abort`**：`CatchPanicLayer` 依赖 `panic = "unwind"`，
    改回后保护会静默失效且没有编译错误（ADR-0004）。
19. 新增超时/容量类配置时必须遵守**逐层递减**：网关 > 请求 > SQL > 单条语句，
    并在 `Config::validate` 里加断言，不要只在注释里写。

### 注释（与代码同等重要）

20. **每个文件必须有文件头注释**：职责 / 边界 / 关键约束（Rust 用 `//!`，Vue 用块注释）。
21. **所有 `pub` 项必须有 `///` 文档注释**，写清「做什么、失败语义、副作用」；`deny(missing_docs)` 会兜底。
22. 注释只写**为什么、约束、坑**，禁止「i 加 1」式翻译注释；注释掉的代码一律删除。
23. `TODO` 必须写成 `// TODO(负责人/模块, 版本): 现状 + 替代方案`，裸 TODO 视为缺陷。
24. **改动行为必须同步改注释**，否则视为未完成（DoD 的一部分）。

---

## 2. 常用命令

```bash
cargo xtask doctor                 # 环境自检（工具链/DB/端口）
cargo xtask new-plugin <id>        # 新建插件（后端 crate + 前端包 + 迁移 + 权限 + CONTEXT.md）
cargo xtask gen <schema.toml>      # schema → 后端四层 + 前端页面 + 迁移 + 权限 + 测试
cargo xtask openapi                # 导出 contracts/openapi.json
cd frontend && npm run gen:api      # OpenAPI → TS 类型
cargo xtask check-perms            # 前后端权限点一致性
cargo xtask arch-check             # 依赖方向 / 文件行数 / unsafe / 生成物哈希
cargo xtask verify                 # 全量验收（fmt/clippy/test/arch/RLS 隔离，提交前必须全绿）

pnpm dev                           # 前端开发
pnpm gen:api                       # 重新生成前端类型
pnpm typecheck && pnpm lint && pnpm test
```

---

## 3. 工作流（严格按序）

```
① Spec     人写 specs/{feature}.md（目标/验收/涉及表/权限点/边界/不做）
② Plan     AI 输出：改哪些文件、新增哪些、风险点、待确认问题
③ Generate AI 执行：xtask gen 出骨架 → 只填 // TODO(ai) 处
④ Verify   cargo xtask verify；失败自行修复，最多 3 轮，仍失败则上报
⑤ Review   人只审：领域规则 / 事务边界 / 权限 / SQL 性能 / 错误码
```

**禁止**：一次生成 20 个文件再验证；禁止跳过 `verify` 声称完成。

---

## 4. 完成定义（DoD）

一个任务只有在以下全部满足时才算完成：

- [ ] `cargo xtask verify` 全绿
- [ ] 新增/修改的权限点已声明，且前后端一致（`check-perms` 通过）
- [ ] 涉及表结构变更时有迁移 + 回滚说明，且已 `sqlx prepare`
- [ ] **新增带 `tenant_id` 的表必须同时建 RLS 策略并 `FORCE`**（`check-rls-coverage` 通过）；
      改了隔离相关代码要跑 `cargo test -p server --test rls_enforced`
- [ ] 有对应的集成测试（`sqlx::test`），覆盖正常 + 异常路径
- [ ] OpenAPI 快照已更新并人工确认无破坏性变更
- [ ] PR 描述包含：改了什么、为什么、影响面、如何验证、回滚方式
- [ ] **行为/配置变更同步更新了文档**（`docs/` 对应章节 + `config/default.toml` 注释）
- [ ] **新增配置项可通过环境变量覆盖**（容器里没有编辑 TOML 的能力）
- [ ] 涉及中间件/启动/停机流程时，同步更新了 `docs/01` §3.1 与 §1.2
- [ ] 涉及架构决策（依赖、panic 策略、限流/阻塞模型、安全默认值）时，已写 ADR 并在总览文档 ADR 表登记

---

## 5. 代码风格（为 AI 可维护性优化）

| 规则 | 说明 |
|---|---|
| 命名唯一 | 只用 `get / list / create / update / delete`，不要 `fetch/query/load` 混用 |
| 显式优于隐式 | 不用宏生成业务逻辑，不做隐式类型转换 |
| 一层抽象 | 不做 `Repository<T>` 泛型套娃 |
| 函数 ≤ 50 行，文件 ≤ 300 行 | 超过即拆分 |
| 注释写"为什么" | "是什么"由代码表达 |
| 错误显式处理 | `?` + `AppError`，禁止吞错 |
| 重复优于过早抽象 | 出现 3 次再抽 |

---

## 6. 绝不能做的事

| 禁止 | 原因 |
|---|---|
| 猜 API / 猜字段 / 猜配置项 | 会编译失败或线上 500；查 `cargo doc` 或代码 |
| 为了让测试通过而改断言 | 掩盖缺陷 |
| 删除或跳过失败的测试 | 同上 |
| 在 `handler` 写业务分支 | 破坏分层 |
| 在事务里调用外部 HTTP | 长事务拖垮连接池 |
| 缓存与 DB 双写 | 数据不一致 |
| 用字符串拼接 SQL / `ORDER BY` | 注入 |
| 跨插件直接查表 | 破坏解耦 |
| 手改 `dist/`、`@generated`、`.sqlx/` | 会被覆盖 / 状态漂移 |
| 提交密钥、Token、生产连接串 | 安全事故 |

---

## 7. 遇到不确定时

1. **先搜**：`docs/`、`AGENTS.md`、插件 `CONTEXT.md`、`docs/adr/`。
   查「某段代码在哪 / 某个能力实现没有」直接看 `docs/10-后端代码地图.md`（有已知差距清单）。
2. **再看样例**：`plugins/demo` 是唯一权威样例，照抄结构而不是发明新写法。
3. **仍不确定**：在 PR 描述里列出疑问，**不要自行决策架构**。
4. 架构决策只能由人做，AI 提供选项与代价分析。
