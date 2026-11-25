# zcadmin 工程化与测试审计

> 审计对象：`D:\fcj\zcadmin`（工作区快照 2026-11-02）
> 审计方式：静态读码 + 可执行探针（`cargo xtask` 探针、`cargo fmt --check`、node 脚本实跑、`cargo --dump-openapi` 产物比对、SHA256 比对）
> 审计约束：**只读**，未修改任何项目代码/配置；仅向 `.audit/` 写入本文件
> 本机环境：Windows 10，rustc/cargo 1.98.1，默认工具链 `x86_64-pc-windows-gnu`（无 gcc）；`stable-x86_64-pc-windows-msvc` 存在但**缺 Windows SDK**（`kernel32.lib` 找不到），因此 `cargo build/check/clippy/test` 均无法执行——详见 §7「未能验证」。

---

## 0. 一句话结论

**工程骨架是真材实料（workspace、插件契约、契约快照、7 个 CI job、非 root 镜像都在），但「门禁」是装饰性的**：
`cargo xtask` 这条命令链**根本跑不起来**（缺 `.cargo/config.toml` alias）、`check-perms` 子命令**不存在**、
`arch-check` **只查 2 条字符串规则**、`#[sqlx::test]` **0 个**、前端 **0 个测试**，
而 CI 里至少**有 2 处确定性红灯**（前端权限扫描脚本 false positive、RLS 断言用超级用户）。

| 维度 | 事实判定 |
|---|---|
| xtask 宣称的 7 条命令 | 6 条子命令存在，0 条能用 `cargo xtask` 调起；`check-perms` 不存在 |
| 测试总量 | 30 个（20 `#[test]` + 10 `#[tokio::test]` + **0 `#[sqlx::test]`**），其中 9 个 `#[ignore]` |
| CI | 7 job；`backend` 与 `integration` 有确定性失败点；无 `cargo xtask verify`、无覆盖率、无依赖审计 |
| 契约 | **已同步**（字节级一致），但插件路径靠手工维护 |
| 文档流程 | `specs/`、`docs/adr/` **不存在**；README 85 个勾选、0 个未勾选 |

---

## 1. 工程门禁实测表（宣称 vs 事实）

### 1.1 总表

| # | 宣称 | 宣称出处 | 实测事实 | 证据（文件:行） |
|---|---|---|---|---|
| G1 | `cargo xtask doctor / verify / arch-check / gen / openapi / new-plugin / check-perms` 全部可用 | `AGENTS.md:64-71`、`README.md:35`、`README.md:236-241`、`README.md:294` | **全部不可用**：仓库无 `.cargo/config.toml`、无 `[alias] xtask`。实测 `cargo xtask --help` → `error: no such command: 'xtask'`（exit 101）。必须写 `cargo run -p xtask -- <cmd>` | 无 `.cargo/` 目录（`Test-Path` = False）；`Cargo.toml` 全文无 alias；探针输出见 §7 |
| G2 | `cargo xtask check-perms` 做「前后端权限点一致性」 | `AGENTS.md:69`、`AGENTS.md:99` | **子命令不存在**。`match` 分支只有 doctor/verify/arch-check/gen/openapi/new-plugin/help，其余一律 `未知命令` | `crates/xtask/src/main.rs:26-48` |
| G3 | `cargo xtask arch-check` 检查「依赖方向 / 文件行数 / unsafe / 生成物哈希」 | `AGENTS.md:70` | 只实现 2 条字符串规则：**R1** 扫 `crates/kernel/Cargo.toml` 中以 `plugin-`/`plugin_` 开头的行；**R2** 在插件 `.rs` 里搜 `plugin_x::` / `use x::` 子串。**不检查行数、不检查 unsafe、不检查 @generated 哈希** | `crates/xtask/src/main.rs:191-247`（R1：196-205，R2：207-239） |
| G4 | `cargo xtask verify` = 全量验收（提交前必须全绿） | `AGENTS.md:71,98`、`README.md:237` | 只串 4 步：`cargo fmt --check` → `cargo clippy -D warnings` → `cargo test --workspace` → `arch_check()`。**不含** OpenAPI 漂移、权限扫描、前端检查、契约校验 | `crates/xtask/src/main.rs:138-163` |
| G5 | `cargo xtask new-plugin` 生成「后端 crate + **前端包** + 迁移 + **权限** + **CONTEXT.md**」 | `AGENTS.md:65` | 只生成 3 个文件：`Cargo.toml`、`src/lib.rs`、`migrations/0001_init.sql`。无前端包、无 `permissions()`、无 `CONTEXT.md`、无测试。且生成的示例表只有 `id/name/created_at`，**缺 `tenant_id`、`created_by`、`deleted_at`、`version`**（违反 `AGENTS.md:36` 铁律 7） | `crates/xtask/src/main.rs:293-398`（迁移模板 386-398） |
| G6 | `cargo xtask gen` = schema →「**后端四层** + **前端页面** + 迁移 + **权限** + **测试**」 | `AGENTS.md:66` | 只生成 2 个文件：1 个 SQL 迁移 + 1 个 `.rs`（模型 + 5 个 handler + router）。无 repo/service/domain 分层、无前端页面、无权限声明、无测试 | `crates/xtask/src/gen.rs:87-116`；渲染器 192-684 |
| G7 | `cargo xtask openapi` 导出契约 | `AGENTS.md:67`、`README.md:46` | **实现存在且产物正确**：内部执行 `cargo run -q -p server -- --dump-openapi` → 校验 JSON → 写 `contracts/openapi.json` | `crates/xtask/src/main.rs:422-452` |
| G8 | `cargo xtask doctor` 检查「工具链、配置与数据库连通性」 | `crates/xtask/src/main.rs:63`、`AGENTS.md:64` | 只打印 rustc/cargo 版本、`config/default.toml` 是否存在、`DATABASE_URL` 掩码值。**不连数据库、不查端口**（与自述不符） | `crates/xtask/src/main.rs:86-120` |
| G9 | `cargo fmt --all --check` | `ci.yml:40` | **实测通过**（exit 0，无输出） | 探针输出见 §7 |
| G10 | `cargo clippy --workspace --all-targets -- -D warnings` | `ci.yml:57` | **未能验证**（构建阻断：缺 `kernel32.lib`） | §7 |
| G11 | `node scripts/check-perms.mjs` | `ci.yml:66` | **实测通过**（"权限扫描通过（12 个插件源文件）"），但语义只是「带 `CurrentUser` 的 handler 体内出现 `require_perm`/`perm::require`/`has_permission`」，**不比对声明集合，也不看前端** | `scripts/check-perms.mjs:41-44` |
| G12 | `node scripts/check-frontend-perms.mjs` | `ci.yml:69` | **实测失败（exit 1）**，且是脚本正则 bug 导致的假阳性 | `scripts/check-frontend-perms.mjs:37`；详情见 §3.2 |
| G13 | `node scripts/check-perm-codes.mjs` / `gen-perm-codes.mjs --check` | `ci.yml:72,75` | **实测通过**（"声明 63 个" / "常量已是最新"） | 探针输出见 §7 |

### 1.2 关键细节：arch-check 到底查了什么

`arch_check()` 全文只有两个循环：

- **R1（`main.rs:196-205`）**：读 `crates/kernel/Cargo.toml`，逐行 `trim()` 后判断 `starts_with("plugin-")` / `starts_with("plugin_")`。只识别「写在行首的依赖名」，`[dependencies.plugin-crm]` 这种写法或缩进的依赖行会漏掉。
- **R2（`main.rs:207-239`）**：遍历 `plugins/*` 目录，对每个插件的每个 `.rs` 文件读成字符串，用 `src.contains("plugin_{other}::")` 或 `src.contains("use {other}::")` 判断跨插件引用。注释、文档、字符串常量里出现同样子串也会误报；`fs::read_to_string(&file).unwrap_or_default()`（`main.rs:218`）在读取失败时静默当成空文件。

**未实现（但 AGENTS 声称有）**：

| 声称的检查 | 是否存在 | 现状反证 |
|---|---|---|
| 文件行数 ≤ 300 | ❌ 无 | `crates/kernel/src/system.rs` **7032 行**、`auth.rs` 1252 行、`plugins/crm/src/api.rs` 1129 行；共 **21 个 .rs**、**10 个 .vue** 超 300 行 |
| 禁止 unsafe | ❌ 无 | 仓库实际无 `unsafe`（仅 `crates/kernel/src/lib.rs:30` 注释提及），但门禁不检查 → 属于「恰好干净」而非「被约束」 |
| `@generated` 文件哈希 | ❌ 无 | 全仓库**没有任何 `@generated` 标记**（`*.rs/*.ts/*.sql/*.vue` 全扫为 0），哈希校验无从谈起 |
| 函数 ≤ 50 行 | ❌ 无 | 无任何检查 |

> 结论：`arch-check` 实际等价于「kernel 的 Cargo.toml 没有 `plugin-*` 行 + 插件源码没有 `plugin_x::` 字面量」。它**检测不到**真正的架构债——例如 kernel 里塞了 7032 行业务代码（`system.rs` 含 92 个 `#[utoipa::path]` 业务接口），`crates/kernel` 事实上已是业务单体，而 R1 仍判「通过」。

### 1.3 xtask 子命令真实行为一览（读码结论）

| 子命令 | 实现位置 | 真实行为 |
|---|---|---|
| `doctor` | `main.rs:86-120` | 打印 rustc/cargo 版本 / 配置文件是否存在 / DATABASE_URL 掩码。不连库 |
| `verify` | `main.rs:138-181` | fmt → clippy → test → arch-check，四步串行；任一步非 0 退出即中止 |
| `arch-check` | `main.rs:191-247` | 仅 R1/R2（见上） |
| `gen <schema.toml>` | `gen.rs:72-117` | 解析 TOML → 校验字段类型 → 写 `migrations/{seq}_create_{table}.sql` + `src/{table}.rs`；打印「还需手工加 `mod` / `.merge()` / 补 OpenAPI paths」 |
| `openapi` | `main.rs:422-452` | `cargo run -p server -- --dump-openapi` → 写 `contracts/openapi.json` |
| `new-plugin <id>` | `main.rs:276-410` | 建目录、写 Cargo.toml/lib.rs/0001_init.sql；打印「还要去 server/Cargo.toml 加依赖 + `use plugin_x as _;`」 |
| `check-perms` | **不存在** | `main.rs:45-47` 落入 `other => Err("未知命令")` |
| `help` | `main.rs:59-70` | 帮助文本里也**没有**列出 `gen`（`main.rs:60-68` 缺 `gen` 条目） |

---

## 2. 测试覆盖清单

### 2.1 总量

| 类型 | 数量 | 位置 |
|---|---:|---|
| `#[test]` | 20 | 见下表 |
| `#[tokio::test]` | 10 | 1 个在 `kernel/src/storage.rs`，9 个在 `apps/server/tests/` |
| **`#[sqlx::test]`** | **0** | 全仓库 0 处（`AGENTS.md:101` 的 DoD 明确要求） |
| `#[ignore]` | 9 | 全部集成测试 |
| 默认 `cargo test --workspace` 实跑 | **21 个**（9 个被 ignore 跳过） | — |
| 前端测试 | **0** | 无 vitest/jest 依赖、无 `test`/`test:unit` 脚本、无 `*.spec.*`/`*.test.*`（`frontend/package.json:7-12`） |

### 2.2 单元测试分布（20 个 `#[test]` + 1 个非 ignore 的 `#[tokio::test]`）

| 文件 | 数量 | 覆盖对象 |
|---|---:|---|
| `crates/kernel/src/mask.rs` | 4 | 字段脱敏纯函数 |
| `crates/kernel/src/audit.rs` | 3 | 审计动作/规则匹配 |
| `crates/kernel/src/password.rs` | 3 | 密码强度/历史校验 |
| `crates/xtask/src/gen.rs` | 3 | 生成器渲染结果字符串断言（不编译生成物） |
| `crates/kernel/src/storage.rs` | 2（1 `#[test]` + 1 `#[tokio::test]`） | 路径穿越防护 / 存储键 |
| `plugins/demo/src/lib.rs` | 2 | 插件 id 校验、路由前缀 |
| `plugins/crm/src/excel.rs` | 2 | Excel 单元格解析 |
| `crates/kernel/src/openapi.rs` | 1 | 契约含核心路径（`build()`，即**不含插件**） |
| `crates/kernel/src/tenant_usage.rs` | 1 | 用量计算 |

### 2.3 集成测试（9 个文件 = 9 个测试函数，全部 `#[tokio::test] + #[ignore]`）

| 文件 | 测试函数 | 覆盖场景 | 规模 |
|---|---|---|---:|
| `apps/server/tests/smoke.rs` | `smoke_end_to_end` | **巨型端到端**：登录→身份→系统读→CRM 写读删→系统写（RLS INSERT/UPDATE WITH CHECK）→指标端点；命中约 **119 条路径**（`/api/auth/*`、`/api/system/*`、`/api/crm/*`、`/api/demo/*`、`/api/openapi.json`） | 3729 行 |
| `apps/server/tests/scope_matrix.rs` | `scope_matrix` | 5 种数据范围（all / dept / dept_and_child / self / custom）+ 多角色并集，仅针对 `GET /api/crm/customers` | 354 行 |
| `apps/server/tests/custom_scope.rs` | `custom_scope_filters_rows` | custom 范围跨部门越权（U∈B，custom={A}，只能看 A） | 309 行 |
| `apps/server/tests/rbac_matrix.rs` | `rbac_matrix` | 最小权限令牌（仅 `crm:customer:list`）打 **8 个 GET + 1 个 DELETE**：预期放行 1、预期 403 共 8 | 152 行 |
| `apps/server/tests/rbac_role.rs` | `rbac_role` | 低权限角色真实登录 → 角色→权限→接口全链路；含字段脱敏、登录锁定、新设备告警、文件下载等直连 SQL 断言 | 671 行 |
| `apps/server/tests/single_session.rs` | `single_session_kicks_previous` | 后登录踢掉前会话（`/me` 变 401） | 2299 B |
| `apps/server/tests/webhook_delivery.rs` | `webhook_delivers_and_signs` | 本地 axum 接收端收 webhook + HMAC 签名校验 | 5903 B |
| `apps/server/tests/webhook_retry.rs` | `webhook_failure_backs_off` | 不可达地址 → `attempts` / `next_notify_at` 退避记账 | 6852 B |
| `apps/server/tests/bench.rs` | `bench_crm_list_and_dashboard` | 性能基线：并发 25、400 请求，`Router::oneshot` 不走 TCP | 5647 B |

**隔离性缺陷**：9 个测试全部 `kernel::app::build(config)` 打同一个 `DATABASE_URL` 指向的库，**没有 `#[sqlx::test]` 的每测试独立事务/独立库**，彼此用 `suffix` 造数区分；`smoke` 里还会调 `/api/system/tenants/purge?retentionDays=0` 与 `bills/overdue/scan` 等破坏性动作。任何一次失败都会污染后续本地复跑。

### 2.4 零覆盖的高风险路径

| 风险面 | 现状 | 证据 |
|---|---|---|
| **权限拒绝** | 仅 9 个端点被验证 403（`rbac_matrix` 的 8 个 GET + 1 个 DELETE），而权限点声明了 **63 个**、`require_perm` 调用 **140 处**。写接口（POST/PATCH/DELETE）的越权几乎全靠 `smoke` 的 happy path | `rbac_matrix.rs:111-121`；`check-perm-codes.mjs` 输出「声明 63 个」 |
| **RLS** | **没有任何测试断言「无租户上下文 → 0 行」**。所有测试用 `DATABASE_URL=postgres://zcadmin:zcadmin@...`（超级用户）连接，**RLS 对超级用户完全失效**；测试里的 `SET LOCAL app.tenant_id` 只是为了让 INSERT 通过策略，不构成 RLS 断言 | 各测试文件首部注释与 `custom_scope.rs:114`、`scope_matrix.rs:272`；CI `ci.yml:98-99,109` |
| **数据范围** | 只覆盖 `crm.customer` 的 **list**。详情、更新、删除、导出、导入、`system` 域（用户/角色/部门）均无范围测试 | `scope_matrix.rs:302-341`（只调 `/api/crm/customers?...`） |
| **并发 / 竞态** | 只有 `bench` 的压测并发与 `single_session` 的「先后登录」。**无**并发写、乐观锁 `version` 冲突、唯一约束竞争、配额并发超发、多实例 advisory lock | 全仓 `tokio::spawn` 仅出现在 `bench.rs:138`、`webhook_delivery.rs:102` |
| **导入导出边界** | 只走 happy path。未测 500 行导入上限、20k 导出行上限、空文件/超大文件、表头缺失、公式注入/CSV 注入、重复行、非法 xlsx | `smoke.rs` 中 `users/import?dryRun=true`、`import-errors` 仅正常路径 |
| **迁移 / 多租户 schema** | 无迁移测试（无「全新库迁移断言」独立用例，靠 `app::build` 隐式执行） | — |
| **生成器产物** | `gen.rs` 只有 3 条字符串包含断言，从不编译生成物 | `gen.rs:748-777` |
| **前端** | 0 测试（无单测、无组件测试、无 e2e） | `frontend/package.json` |

---

## 3. CI 风险

`ci.yml` 共 **7 个 job**（`ci.yml:25,91,160,187,207,224,250`）。

### 3.1 Job 清单与判定

| Job | 做什么 | 判定 | 证据 |
|---|---|---|---|
| `backend` (Rust) | fmt → 数据范围漂移 grep → clippy → `cargo test --workspace` → `cargo run -p xtask -- arch-check` → 3 个 node 权限扫描 → `cargo run -p xtask -- openapi` + `git diff --exit-code` | **确定性红**（见 3.2）；`cargo test --workspace` 只跑 21 个单测 | `ci.yml:25-81` |
| `integration` (PostgreSQL) | 起 `postgres:16-alpine`（`POSTGRES_USER: zcadmin`）→ 依次跑 9 个 `--ignored` 集成测试 → psql RLS 断言 | **确定性红或形同虚设**（见 3.3） | `ci.yml:91-155` |
| `frontend` (Vue) | `npm ci` → `npm run typecheck` → `npm run build` | 大概率绿：**本机实测 `vue-tsc --noEmit` 通过（exit 0）**；`build` 未实测。**无 `npm test` 步骤** | `ci.yml:160-182`；探针见 §7 |
| `docs` (Docs sanity) | 只检查 `plugins/*/CONTEXT.md` 是否存在 | 绿（crm、demo 都有） | `ci.yml:187-202` |
| `docker` | `docker build -t zcadmin:ci .` + `docker compose config` | 未验证；有上下文体积风险（见 3.4） | `ci.yml:207-219` |
| `mobile` (Flutter) | `flutter create --platforms=android` → `pub get` → `analyze` → `build apk --debug` | 未验证（本机无 Flutter SDK） | `ci.yml:224-245` |
| `desktop` (Tauri, windows-latest) | `npm install` → `npm run build`（= `tauri build`，`apps/desktop/package.json:8`）→ 上传 `src-tauri/target/release/bundle/**` | 未验证；本机无 `bundle/` 产物（`Test-Path` = False），说明 `tauri build` 从未成功跑过 | `ci.yml:250-272` |

### 3.2 `backend` 的确定性红灯：前端按钮权限扫描

实跑结果：

```
$ node scripts/check-frontend-perms.mjs
缺少 v-perm 的按钮:
  - frontend\src\views\UsersView.vue: 0"
        size="small"
        style="margin-bottom: 8px"
        @click="exportImportErrors"
      >
        导出错误
EXIT=1
```

根因是**脚本正则缺陷**，不是真实权限缺口：目标按钮写作

```vue
<a-button
  v-if="importResult && importResult.errors.length > 0"
  ...
>
```

`scripts/check-frontend-perms.mjs:37` 的正则 `<a-button\b([^>]*)>([\s\S]*?)<\/a-button>` 用 `[^>]*` 抓属性，遇到 `errors.length > 0` 里的 `>` 就提前截断，于是把后续文本当成按钮文案并跳过 `v-perm` 检查。

**影响**：`ci.yml:68-69` 这一步必红 → `backend` job 必红 → 整个 CI 必红。这与 `README.md:99`「CI 全绿」叙事直接冲突。

### 3.3 `integration` 的 RLS 断言：超级用户 = 自证无效

`ci.yml:147-155`：

```bash
count=$(psql "$DATABASE_URL" -tAc "SELECT count(*) FROM sys.sys_user")
test "$count" = "0"
```

两个问题叠加：

1. **连接用户是超级用户**。`ci.yml:98` 的 `POSTGRES_USER: zcadmin` 在官方 `postgres` 镜像里就是 **superuser**（同理 `docker-compose.yml:15`、`config/default.toml:24`）。PostgreSQL 的超级用户 / `BYPASSRLS` 角色**永远绕过 RLS**，`FORCE ROW LEVEL SECURITY` 只对「表 owner」生效，对 superuser 无效。因此这个断言在语义上**不可能成立**——除非表里真的没数据。
2. **但表里一定有数据**：`app::build` 会 `system::ensure_seed()` 建默认租户与 `admin`（`crates/kernel/src/app.rs:76`），9 个集成测试又灌了大量用户。于是 `count != 0` → `test` 失败 → job 红。
3. **兜底分支也是坏的**：`ci.yml:149-152` 若 `psql` 不存在则 `exit 0` 静默跳过——即「RLS 断言」在最坏情况下**根本没跑**。

> 结论：`README.md:99`「CI 集成任务…含 RLS 断言」在事实层面是「一条必定失败或必定跳过的断言」。整条 RLS 能力（30 张表 FORCE）在默认部署形态下**从未被任何自动化验证过**，且实际处于「形同未启用」状态。

### 3.4 其他 CI 风险点

| 风险 | 说明 | 证据 |
|---|---|---|
| Docker 构建上下文 | `.dockerignore` 只有 `target`（无 `**/` 前缀）与 `**/node_modules`；`apps/desktop/src-tauri/target/` 本机 **3.7 GB / 6184 文件**，`.gitignore` 排了但 `.dockerignore` 没显式排。若构建上下文把嵌套 target 打进去，`docker build` 会极慢甚至超时 | `.dockerignore:2`；`.gitignore:38` |
| Dockerfile 里 `COPY apps ./apps` | 把 `apps/mobile`、`apps/desktop`（含 node_modules）一起拷进编译层 | `Dockerfile:26` |
| 契约漂移检查的判据弱 | 只做 `git diff --exit-code contracts/openapi.json`；不检测**破坏性变更**（删字段/改类型/改必填），也没有语义版本或宽限期机制（`AGENTS.md:43` 要求） | `ci.yml:77-81` |
| 跨平台确定性 | `contracts/openapi.json` 是在 Windows 生成；`utoipa` 的 schema 注册顺序依赖 `preserve_order`/BTreeMap，Linux runner 上重生成理论上存在顺序差异风险（本次无法在 Linux 复核） | `contracts/openapi.json` 二进制比对通过（Windows） |
| 缺的门禁 | 无 `cargo deny` / 依赖白名单（`AGENTS.md:29` 要求）、无覆盖率、无 SBOM、无 `cargo audit`、无前端测试、无 `cargo xtask verify` 调用 | `ci.yml` 全文 |
| CI 与本地命令不一致 | `ci.yml:5` 注释称「与人/AI 本地执行的命令**完全一致**（`cargo xtask verify`）」，但 CI 从不调用 `xtask verify`，且 `verify` 比 CI 少 3 类检查 | `ci.yml:5` vs `main.rs:138-163` |

---

## 4. 文档漂移表

### 4.1 流程资产存在性

| 资产 | AGENTS/文档要求 | 实际 | 证据 |
|---|---|---|---|
| `specs/` 目录 | `AGENTS.md:83`「人写 `specs/{feature}.md`」；`docs/07:283,572` | **不存在**（且 `docs/07:572` 的待办仍是 `- [ ]`，文档自己承认） | `Test-Path specs` = False |
| `docs/adr/` | `AGENTS.md:29`「新增依赖必须先在 `docs/adr/` 提 ADR」 | **不存在**。ADR 只以表格形式散落在 `docs/企业级管理后台-架构设计.md:227-243`（ADR-001~013），没有独立文件、没有状态/日期/决策人 | `Test-Path docs/adr` = False |
| `plugins/*/CONTEXT.md` | `AGENTS.md:19`「没有 CONTEXT.md 的插件先补」 | 只有 `plugins/crm/CONTEXT.md`、`plugins/demo/CONTEXT.md` 两个；`plugins/` 下也只有这 2 个插件，故 CI `docs` job 能过 | 全仓 `CONTEXT.md` 搜索 = 2 |
| `contracts/` | 跨端契约 | `openapi.json` / `errors.json` / `tokens.json` 均在 | — |

### 4.2 README「当前进度」抽查（5 条明显可疑项）

> README 共 **85 个 `- [x]`、0 个 `- [ ]`**（`README.md` 全文统计），即「全部完成」。以下 5 条与代码事实不符。

| # | README 宣称 | 行号 | 事实 | 证据 |
|---|---|---|---|---|
| D1 | `cargo xtask verify / arch-check / new-plugin / doctor` | `README.md:35`（另见 236-241、294） | **命令不可用**：无 `.cargo/config.toml` alias，`cargo xtask` 直接报 `no such command`。README 里 6 条 `cargo xtask ...` 全部照抄会失败 | §1.1 G1 |
| D2 | 「**RLS 完成**：**13 张表** FORCE 生效，PostgreSQL 18 真机冒烟（全新库 + 存量库）通过」 | `README.md:97` | 数字**严重过期**：现在实际是 **30 张表** FORCE（sys 24 + crm 6）；另有 3 张仅 `ENABLE`（`sys_export_task`、`sys_post`、`payment_daily`）、**10 张完全没有 RLS**（`sys_user_role`、`sys_role_permission`、`sys_user_post`、`sys_session`、`sys_tenant`、`sys_tenant_bill`、`sys_tenant_bill_collection`、`sys_tenant_bill_payment`、`sys_tenant_usage_snapshot`、`sys_permission`）。而「真机冒烟通过」在**超级用户连接**下不构成 RLS 证明 | 迁移 `0015_force_rls.sql:10-20`、`0021-0050` 各处、`plugins/crm/migrations/0004_force_rls.sql:7-8`、`0005:39`、`0006:37`、`0007:36`、`0016:58`；`sys_user_role` 定义见 `crates/kernel/migrations/0002_rbac.sql` |
| D3 | 「**CI 集成任务**：PostgreSQL service + 真库冒烟（**含 RLS 断言**）」 | `README.md:99` | RLS 断言用超级用户，**必然失败或静默跳过**；且 `backend` job 因前端扫描脚本必红 | §3.2、§3.3 |
| D4 | 「**CRUD 生成器**：`cargo xtask gen schema.toml` → 迁移 + 模型 + 5 个接口」+「联系人示例：用生成器产出 `contact.rs`（**294 行**）」 | `README.md:84-85` | 生成器**只产 2 个文件**（迁移 + 单文件），不是「四层」；`contact.rs` 实际 **450 行**，且已明显手工改动（与生成模板结构不同）——「用生成器产出」不可复现 | `gen.rs:87-116`；`plugins/crm/src/contact.rs` = 450 行 |
| D5 | 「**桌面端骨架**：Tauri 2 外壳复用 Web 前端」+「**移动端骨架**：Flutter 登录 + 客户列表」；`AGENTS.md` 未提 | `README.md:100-101` | 桌面端确实有 `src-tauri`（`main.rs` + `tauri.conf.json` + 图标），但**从未产出过 bundle**（`bundle/` 不存在），CI 声称「可产出 MSI/NSIS」未经验证；移动端实际有 **13 个页面**（客户/合同/商机/回款/告警…），README 描述为「登录 + 客户列表」是**低报**；但两端都**无测试**、且不属于 Cargo workspace（`Cargo.toml:18-24` 只含 kernel/xtask/plugins/apps/server） | `apps/desktop/package.json:8`；`apps/desktop/src-tauri/tauri.conf.json`；`apps/mobile/lib/pages/*.dart`(13) |

### 4.3 其他文档/注释漂移（抽样）

| 位置 | 文档说的 | 代码事实 |
|---|---|---|
| `crates/kernel/src/tenant.rs:31-33` | 「目前**只有 `configs` 模块使用** `TenantTx`；其余模块仍用 `state.db` + 手写 tenant_id」 | 实际 `system.rs`、`files.rs`、`crm/*` 的 handler 大量以 `mut tx: TenantTx` 取参（如 `gen.rs:473` 的生成模板也是 `TenantTx`）；注释落后于代码（违反 `AGENTS.md:57`「改动行为必须同步改注释」） |
| `crates/kernel/src/metrics.rs:15-18` | 「为什么不用 OpenTelemetry：M4 先解决有没有；**全链路追踪（OTel）在下一阶段接入**」 | `apps/server/src/main.rs:118-162` 已经接入 OTLP/gRPC OTel layer |
| `README.md:95` | 「RLS 第二步：`system.rs` **全部 18 个 handler**」 | `system.rs` 现有 **92 个** `#[utoipa::path]` |
| `docs/07-AI原生开发与持续迭代.md:566-573` | M0 待办清单：`- [ ] 实现 cargo xtask verify`、`- [ ] 建 specs/ 目录` 等 **全部未勾** | 这些勾选状态其实**与事实一致**（槽点在于 README 却全勾）|
| `AGENTS.md:70` vs `main.rs:191-247` | arch-check 查 4 类 | 只查 2 类（见 §1.2） |
| `AGENTS.md:65-66` vs `main.rs:293-398` / `gen.rs:87-116` | 生成器产出前端/权限/测试/CONTEXT | 均未实现 |

---

## 5. 契约一致性（问题 4）

### 5.1 运行时是否包含插件路由：**是**

| 路径 | 用的函数 | 证据 |
|---|---|---|
| `GET /api/openapi.json` → `serve()` | `build_with(&plugins)` ✅ | `crates/kernel/src/openapi.rs:70-73` |
| `GET /docs` → `docs()` | `build_with(&plugins)` ✅ | `openapi.rs:80-83` |
| `server --dump-openapi` | `build_with(&plugins)` ✅ | `apps/server/src/main.rs:47-52` |
| 单元测试 `spec_contains_core_paths` | `build()`（仅内核，**不含插件**） | `openapi.rs:171` |
| 插件路由挂载 | `router.nest("/api/{id}", p.routes())` | `app.rs:105-109` |

`load_plugins()` 靠 `inventory`，而 server 通过 `use plugin_crm as _; use plugin_demo as _;`（`main.rs:22-23`）强制链接，所以插件确实会被注册。

### 5.2 快照与代码是否同步：**是（字节级）**

构建受阻（§7），但有两条硬证据：

1. 仓库内已有可执行产物 `target/debug/server.exe`，直接执行
   `server.exe --dump-openapi > dump.json`，其内容与 `contracts/openapi.json`
   **SHA256 完全相同**：
   - `contracts/openapi.json` = `822D883EEE19ED1D4D62465605F9B16B6C1BF3C57C8F4220D01892B25A03D966`
   - dump 输出 = `822D883EEE19ED1D4D62465605F9B16B6C1BF3C57C8F4220D01892B25A03D966`
   - 均为 **433,344 字节**，`fc /n` 报 `no differences encountered`
2. 结构核对：**119 条 path / 234 个 schema**，其中 `/api/crm/*` **23 条**、`/api/demo/*` **2 条**；路径集合与 `contracts/openapi.json` **完全一致**（`Compare-Object` 双向为空）。

> 可信度说明：该产物 mtime 13:51 晚于全部源码 mtime（最新 ≤ 10:04，`smoke.rs` 10:04:38），且包含 10:02 才新增迁移对应的接口，可判定为「当前源码的最近一次构建」。**无法从源码重新构建**（缺 Windows SDK），因此这是「产物↔快照一致」而非「源码→快照」的完整证明；但结合时间戳可判定二者同代。

### 5.3 契约本身的缺陷

| 缺陷 | 说明 | 证据 |
|---|---|---|
| `tags` 重复 | `system` 出现两次（`openapi.rs:32` 的全局 tag + `SystemApi` 自带 tag） | 导出 JSON 的 `tags` = `auth, system, health, system, crm` |
| 运维端点不在契约 | `/metrics`、`/healthz`、`/readyz`、`/docs` 均未进 OpenAPI | `app.rs:86-103`（路由在册但无 `#[utoipa::path]`） |
| 插件 paths 手工维护 | `CrmApi` 的 `paths(...)` 手写 39 项，`gen.rs:411` 的注释也承认「否则契约里没有它们」；`README.md:324` 的「已知限制 5」自认此事 | `plugins/crm/src/lib.rs:272-284` |
| 无破坏性变更检测 | CI 只 `git diff`，不判断删字段/改类型/改必填 | `ci.yml:77-81` |
| 契约测试只测内核 | `spec_contains_core_paths` 用 `build()`，插件新增/删除接口不会被任何单测发现 | `openapi.rs:169-191` |

---

## 6. 部署与可观测（问题 6）

### 6.1 Dockerfile / compose

| 检查项 | 结论 | 证据 |
|---|---|---|
| 非 root | ✅ `useradd --system --uid 10001 --create-home app` + `USER app` | `Dockerfile:43,53` |
| 多阶段构建 | ✅ backend(rust:1-bookworm) → frontend(node:22-alpine) → debian:bookworm-slim | `Dockerfile:12,30,38` |
| HEALTHCHECK | ✅ 用二进制自带 `--healthcheck`（TCP 探活，容器内无 curl） | `Dockerfile:62-63`；`apps/server/src/main.rs:30-41` |
| 数据目录权限 | ✅ `mkdir -p /app/data && chown -R app:app /app` | `Dockerfile:51` |
| **数据库连接用户是超级用户** | ⚠️ **是**。compose 用 `POSTGRES_USER: zcadmin`（官方镜像建的就是 superuser），应用用同一账号 `postgres://zcadmin:zcadmin@...`。**FORCE ROW LEVEL SECURITY 对超级用户无效**，多租户隔离在默认部署下不生效 | `docker-compose.yml:15,69`；`config/default.toml:24`；`ci.yml:98,109` |
| 密码/连接串 | ⚠️ `Dockerfile:48` `COPY config /app/config`，而 `config/default.toml:35` 明文写着 `dev_password = "admin123456"`、`config/default.toml:24` 明文 DB 口令；compose 用 `${AUTH_DEV_PASSWORD:-admin123456}` 兜底 |
| 加固 | ❌ 无 `read_only`、无 `cap_drop`、无 `security_opt`、无资源 limits、无 PG 非超级用户创建脚本（迁移里也没有 `CREATE ROLE`/`GRANT`） | 全仓 `.sql` 搜 `CREATE ROLE|GRANT|NOSUPERUSER|BYPASSRLS` = 0 命中 |

### 6.2 `/metrics` 鉴权：**无**

- 路由直接挂在根 router 上，handler 只取 `State<AppState>`，**没有 `CurrentUser`、没有权限点、没有任何中间件鉴权**（全局中间件只有 Timeout/Trace/RequestId/metrics/CORS，见 `app.rs:114-132`）。
- 因此 `/metrics` 对公网/同网络任意客户端开放，会泄露 QPS、延迟分布、连接池规模。
- 同时 `metrics.rs` 里 `path` 标签用的是路由模板（`metrics.rs:70-73`），基数控制是**正确的**（这点值得肯定）。

### 6.3 OTel / Prometheus 是否真实接入

| 能力 | 结论 | 证据 |
|---|---|---|
| Prometheus | ✅ **真实接入**。`metrics-exporter-prometheus` 在 `app::build` 首行 `install()`，`/metrics` 输出 text/plain 0.0.4；有 `http_requests_total`、`http_request_duration_seconds`、`db_pool_connections`、`db_pool_idle` | `app.rs:48`；`metrics.rs:32-63,80-93` |
| OTel 链路追踪 | ⚠️ **代码真实接入，但默认关闭**。`main.rs` 用 `opentelemetry_otlp::SpanExporter` + `tracing_opentelemetry::layer()` 导出 OTLP/gRPC，`service_name="zcadmin"`；但 `config/default.toml:54` 的 `otlp_endpoint` 是**注释掉的**，即默认零导出。README「未配置时零开销」属实 | `apps/server/src/main.rs:95-162`；`config/default.toml:51-54` |
| 二者并存 | ✅ 可以共存（一个走 log→OTLP，一个走 metrics registry），但**没有 trace_id 与 metric 的关联**，也没有 span 上的 `tenant_id`/`user_id` 标签 | — |
| 缺口 | 无 `/metrics` 鉴权、无 OTel Collector 配置样例、无 Grafana/Prometheus 抓取配置（`ops/` 只有两个 RLS 开关 SQL） | `ops/` 仅 `force_rls.sql`、`disable_rls.sql` |

---

## 7. 未能验证（环境限制，非结论）

| 项 | 状态 | 原因（原始证据） |
|---|---|---|
| `cargo check/clippy/test --workspace` | **未能验证** | 用指定命令走 MSVC：`cargo +stable-x86_64-pc-windows-msvc run -p server -- --dump-openapi`。`VsDevCmd.bat` 能设好 `link.exe`/`cl.exe`，但链接阶段报 `LINK : fatal error LNK1181: 无法打开输入文件"kernel32.lib"` → 本机**未安装 Windows SDK**（`C:\Program Files (x86)\Windows Kits\10\Lib` 不存在，全盘搜不到 `kernel32.lib`）。默认 GNU 工具链则缺 `gcc`/`dlltool`。故 `clippy -D warnings`（可能因既有 warning 失败）无法判定 |
| `cargo xtask <cmd>` 行为 | **部分验证** | alias 缺失已由 `cargo xtask --help` → `error: no such command: 'xtask'`（exit 101）实证；子命令语义由源码阅读确认，未实跑二进制 |
| `cargo xtask openapi` 重生成 | **间接验证** | 未重建；改用已有 `target/debug/server.exe --dump-openapi`，与快照 SHA256 相同（§5.2） |
| 9 个 `#[ignore]` 集成测试实跑 | **未执行（有意）** | 构建受阻；且本机有 PG 在 5432 监听但**无 `psql` 客户端**。这 9 个测试会写库并含 `tenants/purge?retentionDays=0`、`bills/overdue/scan` 等破坏性调用，属于修改本地数据，超出「只读审计」边界 |
| `npm run build`（前端） | 未运行 | 只跑了 `npm run typecheck`（**通过，exit 0**）；`build` 会改写 `frontend/dist` |
| `docker build` / `docker compose config` | **未能验证** | 本机无 Docker（`docker --version` → 不是内部或外部命令） |
| `flutter analyze` / `build apk` | **未能验证** | 本机无 Flutter SDK（`where flutter` 无结果） |
| `tauri build` | **未能验证** | 无 MSVC + Windows SDK；且无 `bundle/` 历史产物 |

---

## 附录 A：本次实测命令与原始输出摘要

```
# 1) xtask alias
> cargo xtask --help
error: no such command: `xtask`
help: a command with a similar name exists: `test`
[exit 101]

# 2) 格式化门禁
> cargo +stable-x86_64-pc-windows-msvc fmt --all --check
[exit 0，无输出]

# 3) 权限脚本
> node scripts/check-perms.mjs              → 权限扫描通过（12 个插件源文件）  [0]
> node scripts/check-frontend-perms.mjs     → 缺少 v-perm 的按钮: UsersView.vue  [1]  ← CI 红灯
> node scripts/check-perm-codes.mjs         → 权限码校验通过（声明 63 个）       [0]
> node scripts/gen-perm-codes.mjs --check   → 权限常量已是最新                    [0]

# 4) 前端类型
> cd frontend && npm run typecheck
> vue-tsc --noEmit
[exit 0]

# 5) 契约比对
> target\debug\server.exe --dump-openapi > dump.json
> fc /n dump.json contracts\openapi.json
FC: no differences encountered
SHA256 双方 = 822D883EEE19ED1D4D62465605F9B16B6C1BF3C57C8F4220D01892B25A03D966 (433,344 B)

# 6) 构建失败锚点
LINK : fatal error LNK1181: 无法打开输入文件"kernel32.lib"
error: could not compile `serde` (build script) due to 1 previous error
```

## 附录 B：数据点速查

| 指标 | 数值 |
|---|---|
| Rust 测试函数总数 | 30（20 `#[test]` + 10 `#[tokio::test]` + 0 `#[sqlx::test]`） |
| 默认 `cargo test --workspace` 执行数 | 21 |
| 集成测试文件 / 测试函数 | 9 / 9（全部 `#[ignore]`） |
| 前端测试 | 0 |
| `#[utoipa::path]` 注解数 | 176（含 gen.rs 模板 6 处）；契约 path 119 条 / schema 234 个 |
| 权限点声明 / `require_perm` 调用 | 63 / 140 |
| 超 300 行的 .rs 文件 | 21 个（最大 `kernel/src/system.rs` 7032 行） |
| 超 300 行的 .vue 文件 | 10 个（最大 `AuditLogsView.vue` 1341 行） |
| RLS：FORCE / 仅 ENABLE / 无 | 30 / 3 / 10（口径：kernel 迁移建表 36 张 + crm 迁移建表 7 张，含分区表） |
| CI job | 7（backend / integration / frontend / docs / docker / mobile / desktop） |
| README 勾选 | 85 个 `[x]`、0 个 `[ ]` |
