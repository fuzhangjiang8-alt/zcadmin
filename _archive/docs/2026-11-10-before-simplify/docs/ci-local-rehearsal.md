# 本地 CI 演练记录

> 记录规则：每次验证注明日期、范围与环境；新增基础回归不能覆盖早期完整 CI 演练的边界。
> 2026-11-10 文档同步时已将基础功能验收单列，下方历史记录保留当时结论。

## 2026-11-10 文档同步复核（本次）

本次仅更新 README 与 7 份说明文档，未修改实现代码、插件、迁移、生成契约或 AGENTS.md。

| 检查 | 本次结果 |
|---|---|
| 文档本地链接/编码/代码块 | 8 份 Markdown、23 个本地链接检查通过，无替换字符，代码块闭合 |
| 源码对照 | 已核对配置加载/环境覆盖、单端事务、前端会话/字典缓存、用户导入保存点及 xtask 命令 |
| `cargo xtask verify` | 第一阶段 `cargo fmt --all --check` 未通过；涉及 `crates/kernel/src/ssrf.rs`、`plugins/crm/src/perm.rs` |

本次 verify 因格式检查退出，未进入 Clippy、测试及后续门禁；不继承下方历史通过结果。
这两处代码不在文档同步的修改范围内，保留工作区内容，格式调整后需重新验收。
本次未重新执行真库回归、前端构建或生产部署验证。

## 2026-11-10 基础功能验收（上一轮代码完善）

范围：配置、会话、公共错误/分页、文件、基础用户导入和前端身份/查询缓存。
执行环境为 Windows GNU Rust + 本机 PostgreSQL；数据库用例通过 sqlx 创建独立测试库，未复用业务账号。
详细修复及配置说明见 [基础功能报告](./基础功能分析与加固-2026-11-10.md)。

| 检查 | 当时执行结果 | 边界 |
|---|---|---|
| `cargo xtask verify` | 六步通过，含 43 项内核单元测试和 3 项 HTTP 契约测试 | ignored 真库测试、前端构建与契约生成需另外执行 |
| `session_atomicity` | 3 项通过 | 8 路单端并发、失败回滚、多端及租户边界 |
| `file_atomicity` | 3 项通过 | 提交失败保留内容、正常删除、权限拒绝 |
| `user_import_atomicity` | 2 项通过 | SQL 异常行保存点隔离、dryRun 不新增用户 |
| OpenAPI 与 TypeScript | 正式生成器更新成功 | 退出接口响应为 204 / 503；没有手改生成物 |
| `npm test` / `npm run build` | 23 项通过；类型检查及 Vite 构建通过 | 不包含浏览器实点、截图或 E2E 验收 |
| RLS 静态门禁 | 42 张有策略的表均 FORCE | 不等于部署库已迁移，也不替代 owner/普通角色真库验证 |

复现（测试账号需要创建数据库权限；连接串通过环境变量提供，避免写入记录）：

```bash
cargo xtask verify
cargo test -p kernel --test session_atomicity --test file_atomicity --test user_import_atomicity -- --ignored --test-threads=1
cargo xtask openapi
cd frontend
npm run gen:api
npm test
npm run build
```

以下旧表中的 smoke、Webhook、性能及安全基线测试不是本轮全部重跑的结果；
本次文档更新本身不新增这些测试、生产角色或部署验收结论。

## 历史本地 CI 演练（原记录，未在本次整体重跑）

原环境：Windows 10 + PostgreSQL 18 + Node 22；Rust GNU 工具链。
目的：在不依赖 GitHub Actions 的情况下，尽可能复现 `ci.yml` 的各 job。

### 结果总览

| CI Job | 本地结果 | 说明 |
|---|---|---|
| Backend (Rust) | ✅ | fmt / clippy / 单测 / arch-check / 权限扫描 / 契约全部通过 |
| Integration (PostgreSQL) | ⚠️ 8/9 | `webhook_delivery` 因本机回环 TCP 被环境阻断未通过 |
| Frontend (Vue) | ✅ | `vue-tsc` + `vite build` |
| Docs sanity | ✅ | 所有插件均有 `CONTEXT.md` |
| Docker build | ⏭️ 未执行 | 本机无 Docker |
| Flutter build | ⏭️ 未执行 | 本机无 Flutter SDK |
| Tauri desktop | ⏭️ 未执行 | 需要 MSVC + Windows SDK，当前为 GNU 工具链 |

### 集成测试明细

| 测试 | 结果 |
|---|---|
| smoke | ✅ |
| webhook_retry | ✅ |
| webhook_delivery | ❌ 环境限制（见下） |
| single_session | ✅ |
| scope_matrix | ✅ |
| rbac_matrix | ✅ |
| rbac_role | ✅ |
| custom_scope | ✅（本轮修复） |
| bench | ✅ |
| **security_baseline**（2026-11 新增） | ✅ 5 passed + 1 非 ignored（panic 隔离） |
| export_maintenance | ✅（本轮修复测试隔离问题，见下） |

### 发现并修复

#### custom_scope 403

CRM 全面接入权限校验后，测试角色只授予了 `crm:customer:list`，
但用例同时访问商机列表，缺少 `crm:opportunity:list`，返回 403。

修复：测试角色同时授予客户与商机列表权限。

#### webhook_delivery 超时

本机 PowerShell 原生 `TcpListener` + `TcpClient` 回环连接同样超时，
确认是运行环境/防火墙阻断回环 TCP，而非 Webhook 代码问题。
CI 的 Ubuntu runner 不受此限制，仍需以 CI 结果为准。

#### export_maintenance 计数污染（2026-11）

原断言用「租户内某状态的**总行数**」，而开发库会反复运行测试、
其他用例也会留下 `sys_export_task` 行，导致第二次运行必然失败
（观察到 `failed` 从 1 变 2）。

修复：`seed_task` 返回本次插入的 id，断言改为按 id 查询单行状态。
这**收紧了**断言范围（从"整租户计数"到"本次插入的行"），不是放宽；
修后连续运行 3 次均通过。

## 仍待 CI 验证

- Docker 镜像构建与 compose 校验
- Flutter analyze / APK
- Tauri Windows 打包
- webhook_delivery 的真实 HTTP 投递
- 优雅停机（SIGTERM → 摘流 → 排空）：Windows 无 SIGTERM，需在 Linux 容器/CI 验证
