# Rust 代码在哪 / 怎么读（新手导览）

> 你之前在根目录没看到 `.rs` 文件是正常的：这个仓库是 **Cargo Workspace（工作区）**，
> 根目录只放清单和文档，真正的 Rust 源码分散在几个子 crate 里。

---

## 0. 一分钟找到代码

```
D:\fcj\zcadmin\
├── Cargo.toml            ← 工作区清单（不是业务代码，只声明"有哪些 crate"）
├── apps/
│   └── server/
│       ├── Cargo.toml    ← 可执行程序的依赖清单
│       └── src/main.rs   ← ★ 程序入口，从 main() 开始读
├── crates/
│   ├── kernel/           ← ★ 内核：登录、权限、审计、导出、配置……大部分功能在这
│   │   └── src/
│   │       ├── lib.rs    ← 模块清单（一张"目录导航表"）
│   │       ├── app.rs    ← 装配器：把路由/插件/中间件拼起来
│   │       ├── auth_login.rs   ← 登录接口（本文后面逐行讲）
│   │       ├── system_*.rs     ← 用户/角色/菜单/字典等系统管理接口
│   │       ├── tenant_*.rs     ← 租户/计费
│   │       └── audit_*.rs      ← 审计
│   └── xtask/            ← 工程命令：cargo xtask verify / openapi / arch-check
├── plugins/
│   ├── crm/src/          ← CRM 插件（客户、合同、商机、回款……）
│   └── demo/src/         ← 示例插件，最小可照抄的样板
├── config/default.toml   ← 默认配置（数据库地址、端口、会话时长）
├── contracts/openapi.json← 自动生成的接口契约，别手改
├── migrations/ 里的 .sql ← 数据库建表脚本（在 crates/kernel/migrations/）
└── frontend/             ← 前端是 Vue/TypeScript，不是 Rust
```

**一句话**：`apps/server/src/main.rs` 是唯一入口；`crates/kernel/src/` 是主体；`plugins/*/src/` 是业务插件。

---

## 1. 一次请求是怎么走的

以「登录」为例，从浏览器到数据库：

```
浏览器 POST /api/auth/login  {username, password}
   │
   ▼
apps/server/src/main.rs
   main() → kernel::app::build()  把路由表装配好 → axum::serve() 监听 8080
   │
   ▼
crates/kernel/src/auth.rs
   routes() 里 .merge(auth_login::router())  ← 路由表登记了"这个地址归谁处理"
   │
   ▼
crates/kernel/src/auth_login.rs
   login() 函数  ← 真正干活的地方（本文第 3 节逐行讲）
   │
   ├─ TenantTx::begin_tenant()   开一个"带租户上下文"的数据库事务
   ├─ sqlx::query_as(...)        查 sys.sys_user 拿用户行
   ├─ password::verify_password  校验密码
   ├─ load_permissions(...)      查权限点
   ├─ state.sessions.create(...) 写会话（只存令牌哈希，不存明文）
   └─ audit::record_with_meta()  写一条"登录成功"审计日志
   │
   ▼
返回 200 + Set-Cookie（HttpOnly），前端之后靠这个 Cookie 访问其它接口
```

其它接口（用户列表、客户列表、导出……）都是同一套骨架：**路由 → handler → 带租户的事务 → SQL → 返回 `ApiResponse<T>`**。

---

## 2. 五个新手最容易卡住的概念

| 概念 | 在这份代码里是什么 | 为什么这么写 |
|---|---|---|
| `Result<T, E>` + `?` | 几乎所有函数返回 `Result`，出错就 `?` 提前返回 | Rust 没有异常；`?` 等于"出错就把错误交给调用方"，错误最终变成 HTTP 状态码 |
| `async fn` / `.await` | 所有 handler 和数据库调用都是异步 | 网络/数据库要等，等的时候不占线程；`tokio` 是运行时 |
| `axum::extract::State<AppState>` | handler 的第一个参数常是 `State(state)` | 全局共享的数据库连接池、配置，由框架自动注入，不用自己传 |
| `TenantTx` | `TenantTx::begin_tenant(&state.db, tenant_id)` | 每个租户一个数据隔离域；事务里执行 `set_config('app.tenant_id', ...)`，数据库的 RLS 策略据此只放行本租户的行 |
| `#[derive(...)]` | 如 `#[derive(Debug, Deserialize, utoipa::ToSchema)]` | 编译器自动帮你实现"能打印 / 能从 JSON 解析 / 能生成接口文档"的能力，不用手写 |

**RLS（行级安全）** 是这套系统最关键的设计：即使 SQL 忘了写 `WHERE tenant_id = ...`，数据库也会根据当前会话的 `app.tenant_id` 自动过滤，防止看到别家租户的数据。

---

## 3. 逐行读 `crates/kernel/src/auth_login.rs`（已补新手注释）

打开这个文件，从上往下看：

1. `//!` 开头几行是**文件级文档**：说明这个文件负责什么、边界在哪。
2. `use ...` 是导入：把别处的类型/函数拿过来用。
3. `router()` 只做一件事：登记 `POST /api/auth/login` 由 `login` 处理，并挂上限流中间件。
4. `LoginRequest` / `LoginResponse` 是请求和响应的数据结构，`#[serde(rename_all = "camelCase")]` 表示 JSON 里用 `tenantId` 这种驼峰命名。
5. `async fn login(...)` 是主体，函数签名里三个参数由 axum 自动填：
   - `State(state)`：全局状态（连接池、配置）
   - `headers`：请求头（用来取 IP / User-Agent）
   - `Json(req)`：把请求体 JSON 解析成 `LoginRequest`
6. 函数体按「参数校验 → 解析租户 → 查用户 → 校验密码（含失败计数与锁定）→ 计算权限 → 建会话 → 写 Cookie + 审计」的顺序，每一步的失败都统一返回 `AppError::Unauthorized`（401），**故意不区分"用户不存在"和"密码错误"**，防止被拿来猜账号。

文件里已用 `//` 逐段标注了这些步骤，可作为读其它 handler 的模板。

---

## 4. 怎么跑起来 / 怎么验证

```powershell
# 需要 PostgreSQL 已启动，且 .env/环境变量里有 DATABASE_URL
$env:DATABASE_URL = "postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin"
$env:AUTH_DEV_PASSWORD = "admin123456"
cargo run -p server          # 后端，监听 8080

cd frontend; npm run dev     # 前端，localhost:5173
```

常用检查命令（改完代码跑一遍，别凭感觉）：

```powershell
cargo xtask verify      # 格式 + clippy + 单元测试 + 架构规则，一次全查
cargo xtask openapi     # 重新生成 contracts/openapi.json
cargo xtask check-perms # 前后端权限点是否一致
```

---

## 5. 想改点东西，该去哪

| 想做的事 | 打开 |
|---|---|
| 改登录逻辑 | `crates/kernel/src/auth_login.rs` |
| 加一个新接口 | 在对应模块的 `router()` 里 `.route(...)`，再写 `async fn` |
| 改用户管理 | `crates/kernel/src/system_users*.rs` |
| 改客户/合同 | `plugins/crm/src/` |
| 加一张表 | **新增** `crates/kernel/migrations/00XX_xxx.sql`（不要改已发布的迁移） |
| 加权限点 | 迁移 INSERT + 重新生成 `perms.rs` / `permissions.ts` |
| 改前端文案 | `frontend/src/i18n/zh-CN.ts` 和 `en-US.ts`（两边都要加） |

> 仓库铁律写在 `AGENTS.md`：每个文件要有文件头注释、公开项要有文档注释、文件 ≤300 行、禁止 `unwrap()`、写接口必须有权限点、文案走 i18n。
