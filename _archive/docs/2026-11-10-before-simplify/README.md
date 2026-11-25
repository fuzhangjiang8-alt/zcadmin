# 卓才智能

> 企业级管理平台 · 模块化单体 + 插件体系
> 技术栈：Rust（axum + sqlx）+ PostgreSQL 16 + Vue 3.5（Ant Design Vue 4）

---

## 这套工程是什么

一个**可以长期生长的企业技术中台**：内核只提供插件契约、装配器与横切能力，
所有业务（CRM、IM、AI、IoT…）都以插件形式接入，新增业务**不需要改内核**。

架构文档见 [`docs/`](./docs) —— 动手前请至少读：

| 文档 | 为什么读 |
|---|---|
| [架构总览](./docs/企业级管理后台-架构设计.md) | 全局约束、NFR、ADR |
| [后端架构](./docs/01-后端架构.md) | 分层规范、事务、错误模型（规范与目标） |
| [后端代码地图](./docs/10-后端代码地图.md) | **代码现状**：模块在哪、RLS 覆盖、已知差距、改动导航 |
| [基础功能分析与加固](./docs/基础功能分析与加固-2026-11-10.md) | 本轮基础改动、配置覆盖、回归证据与剩余边界 |
| [本地验收记录](./docs/ci-local-rehearsal.md) | 区分历史演练、基础真库回归和未执行的部署验收 |
| [插件体系](./docs/04-插件体系.md) | 写插件必读 |
| [AI 原生开发](./docs/07-AI原生开发与持续迭代.md) | 写代码（或让 AI 写）必读 |
| [AGENTS.md](./AGENTS.md) | AI 工具的最高约束 |

---

## 已实现能力（含历史里程碑）

以下为实现清单；M0/M1 等标记保留阶段来源，不表示当前仍停留在该阶段。
测试结论以注明日期的验收记录为准，不能由勾选状态推断生产可用性。

- [x] Cargo workspace（kernel / xtask / plugins / server）
- [x] 插件契约 + inventory 自动注册 + 拓扑排序
- [x] 配置加载（TOML + 环境变量）
- [x] 统一错误（RFC 9457）与统一响应
- [x] PostgreSQL 连接池 + 每插件独立 schema + 迁移（含 advisory lock）
- [x] 健康检查（`/healthz`、`/readyz`）
- [x] 会话登录闭环（数据库会话、HttpOnly Cookie；单端并发替换与注销失败处理见 `kernel/src/session/`、`auth_session_api.rs`）
- [x] 示例插件（`plugins/demo`）与标准 CRUD 写法
- [x] `cargo xtask verify / arch-check / new-plugin / doctor`
- [x] 前端骨架（登录页 + 自检首页）
- [x] **真实认证**：Argon2id + `sys_user` + 首次启动种子管理员（M1-a）
- [x] **用户管理**：列表 / 创建 / 启停 + 前端页面（M1-a）
- [x] **角色与权限点**：表结构 + 查询接口（M1-a）
- [x] **角色授权闭环**：角色→权限点、用户→角色、全量覆盖保存（M1-b）
- [x] **菜单下发**：后端按权限过滤菜单树 + 前端动态侧边栏（M1-b）
- [x] **字典管理**：类型 + 字典项 + `useDict` 缓存（M1-c）
- [x] **按钮级权限**：`v-perm` 指令 + 后端 `require_perm` 双校验（M1-c）
- [x] **按需引入**：AntD 组件显式注册，vendor 包 gzip 432KB → 250KB（M1-c）
- [x] **字段级脱敏**：用户邮箱等敏感字段按权限打码（rbac_role 验证）
- [x] **OpenAPI 契约**：`/api/openapi.json` + `cargo xtask openapi` + `npm run gen:api` 生成前端类型（M1-d）
- [x] **插件权限点自声明**：`Plugin::permissions()` → 启动时幂等 upsert（M1-d）
- [x] **CI 流水线**：fmt / clippy / test / 架构 / 契约漂移 / 前端构建（M1-d）
- [x] **demo 插件 OpenAPI**：items CRUD + 权限校验进契约
- [x] **用户手机号**：建模 + 格式校验 + 字段级脱敏
- [x] **租户管理**：列表/创建（含初始管理员）/停用启用
- [x] **租户用户配额**：max_users 限制 + 超限拒绝（smoke 验证）
- [x] **租户存储配额**：max_storage_bytes + 上传拦截（smoke 验证）
- [x] **租户软删除**：deleted_at + 会话撤销 + 列表隐藏
- [x] **租户恢复**：恢复已删除租户并重新启用
- [x] **租户物理清理**：按保留期删除并返回用户/文件明细
- [x] **租户定时清理**：后台每 24h 自动执行
- [x] **租户快速登录**：登录页预填租户编码
- [x] **配额看板**：租户列表展示用户/存储使用进度
- [x] **租户详情**：汇总用户/文件/存储/会话/CRM 全量/最近审计
- [x] **租户用量趋势**：每日快照 + 近 30 天 SVG 曲线
- [x] **用量突增告警**：用户/存储日增超 50% 触发
- [x] **租户账单预估**：按用户/存储单价计算月度费用
- [x] **账单 CSV 导出**：按月份导出全部租户账单
- [x] **对账 Excel 导出**：账单/流水/汇总三 sheet
- [x] **逾期账单扫描**：30/60/90 天分级告警（7 天去重）
- [x] **催收记录**：备注/承诺金额/承诺日期
- [x] **月度账单记录**：生成/查询 tenant_bill（草稿）
- [x] **账单状态流转**：draft → issued → paid / void
- [x] **月末自动生成**：每月 1 日生成上月草稿
- [x] **收款流水**：登记收款并自动结清账单
- [x] **退款/冲正**：负向流水并回退账单状态
- [x] **账单对账汇总**：应收/收款/退款/未收
- [x] **账单补生成**：按月份区间补跑草稿
- [x] **默认告警规则**：全新库自动补齐规则
- [x] **配额告警**：用量达 80% 写入审计告警（smoke 验证）
- [x] **导入支持手机号**：Excel 模板新增手机号列
- [x] **导入模板下载**：前端一键下载示例 xlsx
- [x] **导入预校验**：dryRun 模式只校验不写库
- [x] **导入文件内查重**：重复用户名逐行报错
- [x] **导入错误导出**：错误行 xlsx（含原始字段）
- [x] **CRM 插件（客户管理）**：独立 schema、权限点自声明、数据权限、软删、审计（M2）
- [x] **插件菜单自声明**：`Plugin::menus()` → 启动时按租户幂等落库（M2）
- [x] **CRUD 生成器**：`cargo xtask gen schema.toml` → 迁移（含 RLS FORCE）+ 模型 + 5 个接口（权限点 + 审计）
- [x] **联系人示例**：用生成器产出 `plugins/crm/src/contact.rs`（294 行，含数据权限）
- [x] **文件管理**：本地对象存储、multipart 上传、下载、软删、路径穿越防护（M3）
- [x] **后台任务**：tokio interval 定时清理过期会话（M3 简化版，后续换 apalis）
- [x] **通知中心**：站内通知发布/列表/未读数/已读（含顶部铃铛轮询）
- [x] **Excel 导入导出**：客户导出 xlsx、导入逐行校验并返回失败明细（M3）
- [x] **部门管理**：组织树 + 新建部门（数据权限的依据）
- [x] **审计日志查询**：按动作过滤、分页查看（仅追加，不可改）
- [x] **参数配置**：`sys_config` + 公开配置下发（`is_public` 控制）
- [x] **RLS 第一步**：`TenantTx` 请求级事务 + `sys_config`/`sys_notice` 策略（未 FORCE）
- [x] **RLS 第二步**：`system.rs` 全部 18 个 handler 迁移到 `TenantTx`，补齐组织/角色/字典/审计策略
- [x] **RLS 第三步**：`files` / `crm` / Excel 全部迁移，业务表策略就绪（仍未 FORCE）
- [x] **登录租户解析**：多租户登录需填租户编码；单租户可留空
- [x] **RLS 完成**：40 张业务表全部 FORCE（其余为全局表/会话表/迁移表），PostgreSQL 18 真机冒烟（全新库 + 存量库）通过
- [x] **前端插件注册表**：CRM 页面迁入 `src/plugins/crm`，路由自动展开
- [x] **CI 集成任务**：PostgreSQL service + 真库冒烟（含 RLS 断言）
- [x] **桌面端骨架**：Tauri 2 外壳复用 Web 前端（`apps/desktop`）
- [x] **移动端骨架**：Flutter 登录 + 客户列表（`apps/mobile`）
- [x] **OTel 链路追踪**：OTLP/gRPC 导出，未配置时零开销
- [x] **登录失败锁定**：5 次失败锁定 15 分钟（真库冒烟覆盖）
- [x] **菜单管理**：可视化增删改、防环校验、隐藏菜单（补齐若依最后一块）
- [x] **异步导出**：任务表 + 后台生成 + 轮询下载（审计/用户，20k 行上限）
- [x] **用户导入导出**：Excel 批量导入（500 行上限，逐行错误）+ 异步导出
- [x] **导出任务页**：任务列表、自动刷新、重新下载、删除（含清理文件）
- [x] **Prometheus 指标**：`/metrics`（请求量/延迟/连接池）
- [x] **单体交付**：后端托管前端静态资源（`web_dist`）+ Dockerfile + compose
- [x] **生成器 OpenAPI**：生成的 CRUD 自动带 utoipa 注解，联系人接口已进契约
- [x] **多租户 RLS**：schema 隔离 + RLS FORCE（scope_matrix 验证）
- [x] **平台/租户权限分域**：租户管理员不再是平台超管；`system:tenant:*` 仅平台可见/可授（tenant_isolation 验证）
- [x] **关联表 RLS**：`sys_user_role` / `sys_role_permission` / `sys_user_post` 按父表可见性隔离
- [x] **会话撤销**：禁用用户、改角色、改权限后吊销相关会话并清本实例缓存；其他实例最长 30 秒后生效
- [x] **回款软删冲减**：`contract.paid_cents` 与 `payment_daily` 不再虚增（含历史回填）
- [x] **账单草稿守卫**：重复生成不会覆盖已出账/已收款账单
- [x] **导入行级 SAVEPOINT**：单行冲突不再让整批事务中止
- [x] **`cargo xtask` 命令链可用**：`.cargo/config.toml` alias + `check-perms` 子命令
- [x] **登录限流**：IP 维度 60 次/分钟（`LOGIN_RATE_LIMIT_PER_MIN` 可调），429 + Retry-After
- [x] **CRM 多实体导入 SAVEPOINT**：合同/回款/联系人/商机导入单行失败不再整批回滚
- [x] **写侧父实体校验**：联系人/商机/合同/回款必须挂在数据范围内可见的客户/合同上
- [x] **导出脱敏**：用户导出按 `system:user:view_sensitive` 决定是否打码邮箱
- [x] **租户外键校验**：建用户/建部门/角色部门范围都校验 `tenant_id` 归属

---

## 快速开始

### 0. 前置

| 依赖 | 版本 | 说明 |
|---|---|---|
| Rust | 1.85+ | 本机为 `x86_64-pc-windows-gnu`，需要 MinGW 的 `dlltool` |
| PostgreSQL | 16+ | 或用 `docker compose up -d postgres` |
| Node.js | 22 系列较新版本或 24 | 前端构建与 `npm test`；测试使用类型擦除及 `node:module.registerHooks`，CI 使用 Node 22 |

> **Windows + GNU 工具链注意**：如果 `cargo build` 报
> `error calling dlltool 'dlltool.exe': program not found`，
> 说明 MinGW 不在 PATH。把 `C:\msys64\mingw64\bin` 加到 PATH 即可
> （或用 `rustup default stable-x86_64-pc-windows-msvc` 切到 MSVC 工具链）。

### 1. 起数据库

```bash
docker compose up -d postgres
```

没有 Docker 的话，准备一个 PG 实例并设置：

```bash
set DATABASE_URL=postgres://user:pass@localhost:5432/zcadmin
```

### 2. 起后端

```bash
cargo run -p server
# 或在 Windows 上一键（自动补 MinGW 路径）
powershell -ExecutionPolicy Bypass -File scripts/dev.ps1
```

验证：

```bash
curl http://localhost:8080/healthz     # {"status":"ok",...}
curl http://localhost:8080/readyz      # {"status":"ready"}
```

### 3. 起前端

```bash
cd frontend
npm install
npm run dev
# 打开 http://localhost:5173
```

默认账号（**仅开发环境**，生产必须用环境变量覆盖）：

```
admin / admin123456
```

---

## 性能基线

本机 PostgreSQL 18，并发 25、请求 400，走 `Router::oneshot`（不经过 TCP）。
数据量：5 万条回款、约 100 个客户。

| 场景 | debug RPS / p95 | release RPS / p95 |
|---|---|---|
| 客户列表（20 条/页） | 893 / 34.3ms | **1687 / 16.0ms** |
| 经营看板（按日汇总） | 1040 / 36.3ms | **1701 / 18.6ms** |
| 合同回款（预聚合 + 分页 50） | 209 / 128ms | **457 / 98.2ms** |

release 构建约 **1.9~2.2 倍吞吐**、p95 降低约一半（本次构建耗时 7m46s）。

复现：

```powershell
powershell -ExecutionPolicy Bypass -File scripts/bench.ps1
psql "$env:DATABASE_URL" -f scripts/seed-bench.sql   # 可选：造 5 万回款
```

### 优化路径

1. 列表分页 + 复合索引：客户列表稳定在 800+ rps；
2. 聚合无法靠索引消除（`sum()` 扫 5 万行 12~18ms）→ **预聚合**：
   `contract.paid_cents`（触发器增量维护）、`payment_daily`（按日汇总）；
3. 索引排序必须与 `ORDER BY` 完全一致（含 `NULLS LAST`），修正后明细查询 51ms → 0.37ms；
4. 明细接口真分页（默认 50、最大 200），响应体不随数据量膨胀。

## 生产部署（Docker）

```bash
# 构建并启动（API + 前端 + PostgreSQL）
AUTH_DEV_PASSWORD='请改成强密码' docker compose up -d --build

# 访问
open http://localhost:8080
```

镜像内是**单个二进制**：Rust API 直接托管 Vue 构建产物（`WEB_DIST`），
没有 Nginx、没有 Node 运行时。

本地没有 Docker 时，可先跑静态校验（COPY 源、compose、dockerignore）：

```powershell
powershell -ExecutionPolicy Bypass -File scripts/check-dockerfile.ps1
```

真正的镜像构建由 CI 的 `docker` job 完成（`ubuntu-latest` 自带 Docker）。

可观测：

| 端点 | 用途 |
|---|---|
| `/healthz` | 存活探针（不查依赖；停机排空期间仍返回 200） |
| `/readyz` | 就绪探针（停机中或数据库不可用时 503） |
| `/metrics` | Prometheus 指标（请求量/延迟/连接池/饱和标记） |

### 生产必设环境变量

| 变量 | 必填条件 | 默认 | 说明 |
|---|---|---|---|
| `AUTH_DEV_PASSWORD` | 生产必填 | `admin123456` | release 构建会校验强度，弱口令直接启动失败 |
| `TRUSTED_PROXIES` | 网关/负载均衡之后必填 | 空 | CIDR 逗号分隔，如 `10.0.0.0/8`。为空时**不采信** `X-Forwarded-For`，审计里记的是代理 IP |
| `METRICS_TOKEN` | `/metrics` 可被外部访问时必填 | 空（开放） | 设置后需 `Authorization: Bearer <token>` |
| `EXPOSE_API_DOCS` | 面向公网时设为 `false` | `true` | 关闭 `/docs` 与 `/api/openapi.json` |
| `LOG_FORMAT` | 接入日志平台时 | `pretty` | 设为 `json`（容器镜像已默认 json） |
| `DB_MAX_CONNECTIONS` | 多实例部署时 | `20` | 实例数 × 池上限不能超过 PG 连接预算 |
| `MAX_CONCURRENCY` | 大机型可上调 | 核数×8，≥32 | 并发上限，超出立即 503 |
| `SHUTDOWN_GRACE_SECS` | 一般不用改 | `20` | 必须小于编排系统的 `stop_grace_period` |
| `STATEMENT_TIMEOUT_SECS` | 一般不用改 | `5` | 必须小于请求超时；在建连时下发到 PG |
| `BLOCKING_MAX_CONCURRENCY` | 大内存机型可上调 | `4` | Argon2/xlsx 并发数，**每并发约 19MiB** |
| `LOGIN_RATE_LIMIT_PER_MIN` | 按需 | `60` | 每 IP 每分钟登录尝试上限 |
| `OTEL_TRACES_SAMPLER_ARG` | 排障时 | `0.1` | 临时调到 `1.0` 做全量追踪，事后改回 |
| `CONFIG_PATH` | 容器 | `config/default.toml` | 配置文件路径 |

基础配置变量、默认值与校验约束见 [基础功能报告](./docs/基础功能分析与加固-2026-11-10.md#部署配置)
及 `config/default.toml`；安全部署要求见 `docs/05-安全合规与运维.md` 第 7 节。
`Config` 解析的数字/布尔变量格式错误会中止启动；请求超时必须分别大于获取连接和 SQL 超时，后两者必须大于零。

---

## 常用命令

```bash
cargo xtask doctor            # 工具链/配置路径/连接串存在性检查，不实际连接数据库
cargo xtask verify            # 六步：fmt + clippy + test + 架构 + RLS 静态 + 权限/错误码契约
cargo xtask check-perms       # 权限声明、前端按钮、权限常量与错误码一致性
cargo xtask arch-check        # 架构规则：内核不依赖插件、插件不互相引用
cargo xtask new-plugin crm    # 生成插件骨架
cargo xtask gen schema/crm/contact.toml  # 从 schema 生成 CRUD
cargo xtask openapi           # 导出 contracts/openapi.json
cargo test --workspace        # 普通测试；不执行标记为 ignored 的真库用例
cargo run -p server           # 启动后端
```

基础真库回归（先将 `DATABASE_URL` 指向测试 PostgreSQL；账号需要创建数据库权限，sqlx 自动创建/清理隔离测试库）：

```bash
cargo test -p kernel --test session_atomicity --test file_atomicity --test user_import_atomicity -- --ignored --test-threads=1
```

`verify` 不包含真库/RLS 角色隔离实测、前端构建或 OpenAPI 重新生成；这些检查应分别执行。

前端（在 `frontend/` 中）：

```bash
npm run dev         # 开发
npm run typecheck   # 类型检查
npm test            # Node 内置测试：请求、会话、权限、字典/i18n 等
npm run build       # 生产构建
npm run gen:api     # 已导出的 OpenAPI → src/types/api.d.ts；禁止手改生成物
```

---

## 目录结构

```
zcadmin/
├── AGENTS.md                  # AI 工具约束（铁律 / 命令 / DoD）
├── Cargo.toml                 # workspace：依赖与构建配置
├── docker-compose.yml         # 本地 PG + 应用（含 stop_grace_period）
├── config/default.toml        # 默认配置（环境变量可覆盖）
├── docs/                      # 架构文档集（01–10 + adr/）；后端代码导读见 10
├── contracts/                 # 跨端契约：错误码 / 设计 token
├── crates/
│   ├── kernel/                # 内核：插件契约 + 装配器 + 横切能力
│   │   ├── migrations/        # sys schema 迁移
│   │   ├── tests/            # HTTP 契约与隔离真库基础回归
│   │   └── src/
│   │       ├── app.rs         # 装配器（启动流程 + 中间件顺序）
│   │       ├── plugin.rs      # Plugin trait + 注册表 + 拓扑排序
│   │       ├── auth.rs        # 会话与登录
│   │       ├── client_ip.rs   # 可信代理与客户端 IP 解析
│   │       ├── blocking.rs    # 受控阻塞池（Argon2 / xlsx）
│   │       ├── db.rs          # 连接池 / schema / 迁移 / statement_timeout
│   │       ├── error.rs       # 统一错误（problem+json）
│   │       ├── response.rs    # 统一响应与分页
│   │       ├── config.rs / config/  # 配置类型、环境覆盖与启动校验
│   │       └── session.rs / session/ # 会话查询、事务替换与有界缓存
│   └── xtask/                 # 工程命令
├── plugins/
│   ├── demo/                  # 示例插件（黄金样例）
│   └── crm/                   # 第一个业务插件：客户管理
│       ├── migrations/
│       └── src/lib.rs
├── apps/
│   └── server/                # 唯一可执行入口
├── frontend/                  # Vue 3 管理后台
└── scripts/                   # 开发脚本
```

---

## 新增一个插件（三步）

```bash
cargo xtask new-plugin crm
```

1. 生成 `plugins/crm/`（Cargo.toml + lib.rs + migrations）
2. 在 `apps/server/Cargo.toml` 加一行：`plugin-crm = { path = "../../plugins/crm" }`
3. `cargo run -p server` —— 路由自动挂到 `/api/crm`，迁移自动执行

**内核代码零改动。** 这就是插件化的验收标准。

---

## 设计要点（为什么这么写）

| 决策 | 原因 |
|---|---|
| 每插件独立 PG schema | 迁移互不阻塞，卸载 = `DROP SCHEMA` |
| 编译期插件（非 dylib） | Rust 无稳定 ABI，动态库跨版本会 UB |
| 统一 `AppError` + problem+json | 前端只需一个拦截器；5xx 不泄漏内部信息 |
| HttpOnly Cookie 会话 | localStorage 存 token 等于把 XSS 变成账号接管 |
| 全链路 UUIDv7 | 时间有序、分布式友好、不泄漏数据规模 |
| `xtask verify` 一条命令 | 人、CI、AI 用同一种方式验收 |

---

## 已知限制

1. **会话缓存 30 秒**：多实例吊销存在最长 30 秒窗口。缓存是**进程内**的，写入失效只清本实例，其他实例要等 TTL 过期。
2. **字段级脱敏覆盖邮箱与手机号**：其他敏感字段按需扩展。
3. **多租户已启用 schema + RLS**：管理员可创建/停用租户、设置配额并管理账单与收款；自助注册开通仍属演进目标。容器应用连接必须使用 `zcadmin_app`（`NOSUPERUSER NOBYPASSRLS`），超级用户会绕过 FORCE RLS；已有数据卷应按 `ops/init-app-role.sql` 补建角色并核对授权，保留业务数据。
4. **CI 已配置，部分任务未本地演练**：Docker / Flutter / Tauri 及 webhook_delivery 受环境限制，详见 `docs/ci-local-rehearsal.md`。
5. **插件 OpenAPI 需手工维护**：新增接口若忘记注解不会自动进契约。
6. **Flutter 端未在本机构建**：本机无 Flutter SDK，告警规则页需由 CI 的 mobile job 验证。
7. **性能基线不是容量结论**：`apps/server/tests/bench.rs` 是 25 并发、走 `Router::oneshot`（不经 TCP），
   只能做回归对比；NFR 里的 500 并发 / 1000 QPS 必须用 k6 + 真实网关验证，尚未做过。
8. **慢查询/错误请求未做 100% 采样**：OTel 目前统一按 `sample_ratio`（默认 10%）采样，
   文档中「错误与慢请求全采样」需要自定义 Sampler，尚未实现。
