# AGENTS.md · 仓库协作指南

适用于 `D:\fcj\zcadmin`。本仓库是 Rust 2021 / Axum / sqlx / PostgreSQL + Vue 3 / TypeScript 项目，采用模块化单体与编译期插件；MSRV 以 Cargo.toml 为准，当前为 1.85。
本文件规定开发约束。以用户当前明确的任务范围为准；架构目标、历史记录与真实实现须区分，发现不一致先查代码，不猜 API 或另建框架。

## 1. 开工与阅读

- 先检查工作目录和 `git status --short`；若没有 `.git`，如实说明，禁止自行初始化或编造提交/差异。保留他人及并行改动，不执行无授权的重置、覆盖或大范围格式化。
- 通读 [架构总览](docs/企业级管理后台-架构设计.md)、[开发流程](docs/07-AI原生开发与持续迭代.md)、[注释规范](docs/09-注释与文档规范.md)；从 [代码地图](docs/10-后端代码地图.md) 定位实际入口。
- 改后端先读 [01 §4 分层](docs/01-后端架构.md#4-分层规范)，改前端先读 [02 §0 UI边界](docs/02-前端架构.md#0-ui-选型与封装边界)；数据库/部署分别查 [03](docs/03-数据架构.md)、[05](docs/05-安全合规与运维.md)。
- 涉及插件先读目标 `plugins/<id>/CONTEXT.md`；缺失先补术语、表、权限、状态与边界。`plugins/demo` 是最小接入样例，但其存量实现不代替下述分层约束。
- 不把 `_archive/` 的旧接口、伪代码或测试结果作为当前实现。基础功能任务不顺手优化插件、升级依赖或重构无关模块。

## 2. 真实工程入口

| 位置 | 职责 |
|---|---|
| `apps/server/src/main.rs`、`crates/kernel/src/app.rs` | 配置/日志、插件链接、装配、中间件、监听与停机 |
| `crates/kernel/src/` | 身份、租户、RBAC、审计、文件、错误及运行基础设施 |
| `crates/kernel/migrations/`、`crates/kernel/tests/` | 内核迁移、HTTP契约及独立真库回归 |
| `plugins/*`、`apps/server/tests/` | 业务实现与独立迁移；全链路/权限/隔离等集成测试 |
| `frontend/src/`、`frontend/tests/` | 单包Vue页面、features、公共请求与缓存；Node内置测试 |
| `config/`、`schema/`、`contracts/`、`crates/xtask/` | 部署配置、生成输入、跨端契约与工程命令 |

前端实际使用 npm、vue-query、Vue ref 和 Ant Design Vue；当前没有 `@admin/*` 包或对应路径别名，也没有 Pinia、Vitest、通用 `lint` 脚本。不要为满足目标目录臆造包或新增依赖。
SDK/UI分包仍是既定方向：复用现有公共入口和组件，在当前结构内做最小修改，不新增无关组件库耦合；分包迁移另行评审。

## 3. 架构与实现规则

- `kernel` 不依赖任何插件；插件不得直接互相 `use` 或跨域直接写表。通过公开契约/已落地事件能力协作，不把 `depends_on` 当作直接依赖授权。
- 新业务按 handler → service → repository/domain 分层；handler只处理HTTP，service管理业务/事务，repository处理参数化SQL。存量handler SQL仅做任务内必要修正，不扩大该模式，也不借机整仓重构。
- 复用 `AppState`、`CurrentUser`、`TenantTx`、`scope::condition`、权限常量、`AppError` 和响应类型。只有代码中存在的接口才能调用；不要假设已有 Outbox、Refresh、MFA、通用生命周期钩子或持久队列。
- 标准JSON响应走 `ApiResult<T>` / `ApiResponse<T>`；文件、204等按现有HTTP语义处理。错误映射 `AppError`，5xx详情只入日志，禁止向客户端泄漏SQL、路径、堆栈或anyhow链。
- 写接口必须校验权限并登记到相应声明中；插件在 `permissions()` 声明，内核按现有系统声明入口维护。详情/导出也需资源归属与数据权限；越权资源返回404，无功能权限返回403。
- 字段、状态、字典、URL及参数先查源码/迁移/OpenAPI；破坏性契约变更先版本化或给宽限期，接口/错误码变更同步正式契约和前端类型。
- 禁止unsafe，非测试代码不新增unwrap/expect或吞错。函数目标≤50行、页面/源码文件≤300行；超过优先拆真实职责，不做空泛抽象或无关风格清洗。
- 新第三方依赖、安全默认值、插件契约/存储/运行模型变化，先写ADR并取得明确批准；检查真实依赖与工具配置，不宣称未配置的cargo-deny白名单或lint已生效。

## 4. 数据、并发与安全底线

- 不修改已发布迁移，只新增；业务tenant表应有 `tenant_id/created_at/created_by/deleted_at/version` 及配套策略、ENABLE、FORCE RLS。全局/会话表等例外沿用既有安全模型，不随意增删策略。
- 租户业务使用 `TenantTx`，上下文必须事务级 `set_config(..., true)`。功能权限、租户隔离、部门/本人/custom范围分别校验，平台超管不等同租户管理员，收窄PAT不能恢复超管权限。
- SQL值绑定参数，动态标识符/排序取内部白名单；分页/批量/上传有上限。新增慢查询给出解释计划或明确验证缺口，不能只以编译通过证明SQL正确。
- Argon2、Excel、图片及重同步操作走 `kernel::blocking::run`；Workbook构造用 `kernel::xlsx::run` / `run_anyhow`，异步文件I/O优先tokio::fs。禁止直接spawn_blocking绕过限流，不因处于tokio::spawn就直接执行重任务。
- 客户端IP只取 `client_ip::ClientMeta`；禁止业务直接读XFF/X-Real-IP。release保持 `panic = "unwind"`，维护CatchPanic请求隔离。
- 过载使用503/Unavailable，限流使用429/RateLimited，保留Retry-After。外部HTTP设置超时/有界重试，用户可控URL复用ssrf校验并考虑出口网络限制；禁止事务内等外部HTTP。
- 新配置支持环境变量覆盖，在Config加载/校验链落实范围与预算：网关>请求，取连接与SQL超时均非零且分别小于请求。秘密只通过环境/Secret注入，禁止硬编码或写入日志/文档。

修改以下能力必须保留已落地的一致性规则：

| 能力 | 不可破坏的行为 |
|---|---|
| 单端登录 | 同事务按tenant/user加锁后替换，插入失败保留旧会话 |
| 会话撤销/缓存 | 失效代次防旧查询回填；只清本实例，其他实例最长有30秒窗口，不宣称全局即时撤销 |
| 文件删除 | 先提交元数据软删，再删物理内容；提交失败保留原文件，物理删除失败留可定位记录 |
| 用户导入 | 最多500行、每行SAVEPOINT；错误行回滚、成功行最终提交；dryRun不新增用户，错误报告不含初始密码 |

## 5. 前端要求

- 仅用 `<script setup lang="ts">`，保持严格类型；页面复杂交互拆 `features/`，通用展示放 `components/`，禁止扩散跨插件引用。
- 列表/详情/字典等服务端数据用vue-query及共享 `api/query-client.ts`；局部交互/当前身份沿用ref，不引入新状态框架或自建第二个查询缓存。
- 请求优先复用 `api/client.ts` 的request/http；保留credentials、HeadersInit、错误状态和AbortSignal。FormData交浏览器生成boundary，不存token到localStorage，不自动重放非幂等写请求。
- 退出/重新登录调用 `clearSession()` 清身份与查询/mutation缓存；保留旧/me请求代次保护，字典查询传递取消信号，禁止跨账号/租户回填旧数据。服务端撤销失败不得假退出。
- 文案走i18n，中英文键一致；v-perm和路由校验不替代后端鉴权。隐藏/禁用/加载/失败态及危险操作确认要完整。

## 6. 生成器与插件接入

真实入口在 `crates/xtask/src/main.rs` 和 `gen.rs`：

| 命令 | 实际范围 |
|---|---|
| `cargo xtask new-plugin <id>` | 后端crate、lib.rs和迁移骨架；CONTEXT、前端、权限/菜单和装配需核对补齐 |
| `cargo xtask gen <schema.toml>` | 迁移及Rust模型/CRUD路由，提示挂载；不自动生成完整四层、页面与全部测试 |
| `cargo xtask doctor` | 工具链版本、配置/连接串存在性，不实际验证DB连通/端口 |
| `cargo xtask arch-check` | kernel依赖、插件源码引用及指定业务目录单文件≤300行；不检查全部函数长度/unsafe/生成物hash |

执行gen前检查目标路径：现有实现会写Rust目标文件并新增迁移，不能盲目重跑覆盖已有业务或重复建表。按产物核对权限、路由、OpenAPI及RLS，不假定骨架已满足DoD。
`@generated`、OpenAPI/TS、权限常量、`.sqlx/`、`dist/` 不手改；修改真实来源再用正式生成器。若生成链无法安全再生，明确阻塞，不擅自删标记或覆盖。
新插件还需server的可选依赖、Cargo features、main显式链接、前端注册；运行期 `DISABLED_PLUGINS` 只是启动过滤，禁用不删除数据、不等于热卸载。

## 7. 工作流与验证

先给目标/边界/验收规格和最小文件计划，再小步实现/验证，最后复查领域规则、事务、权限、SQL性能和错误码；不一次生成20个文件后才验证。
缺生成器可按现有分层手写；不能为遵守旧示例臆造TODO区、宏或公共API。

```bash
# 仓库根目录；修改代码后必须执行，失败不能声称完成
cargo xtask verify
# 改接口/错误码/权限时
cargo xtask check-perms
cargo xtask openapi

# frontend/ 下：按前端/契约改动执行
npm run gen:api
npm test
npm run build
```

verify依次执行fmt、clippy、普通测试、依赖边界、RLS静态覆盖、权限/错误码检查；不执行ignored真库、前端构建或契约生成。build包含vue-tsc；Node内置测试不等于浏览器/E2E/覆盖率验收。
涉及事务/会话/文件/导入，补跑 `crates/kernel/tests/` 对应用例；sqlx::test需测试DATABASE_URL和建库权限。server集成测试会改指定库，必须用专用测试库，禁止指向生产。
涉及RLS，按CI准备NOSUPERUSER/NOBYPASSRLS的表owner与普通角色并执行rls_enforced；静态门禁或超级用户测试不代替角色隔离。完整命令见 [07](docs/07-AI原生开发与持续迭代.md)。
失败先区分环境、既有/并行问题与本次回归；仅修任务范围，最多3轮，仍失败报告命令、阶段和原因。禁止改断言、删/跳测试或自动格式化无关代码凑全绿。
Windows GNU需MinGW在PATH。文档任务检查链接/锚点/编码，运行verify后如实记录阻塞，不扩大到无关实现修改。

## 8. 注释、文档与交付

每个源码文件有职责/边界/关键约束文件头；所有pub项写///，公开函数说明功能、参数/返回、失败与副作用；私有复杂转换/状态/SQL条件写简明中文注释。
TODO统一 `TODO(负责人/模块, 版本): 现状与替代方案`；禁止翻译代码、注释掉的实现、过期承诺及裸TODO。行为/配置变更同步注释、对应文档和默认配置说明。
文档分工：README管入口，01～05管规则/部署，07管命令，10管定位，ci-local-rehearsal管带日期的实际结果；一次性过程归档，不再向规范追加流水账。
完成需说明改动/原因/影响、实际测试与未测项、风险/回滚。新增DB结构附迁移/恢复方案，涉及离线SQL元数据再执行prepare；不把所有运行时sqlx查询写成已受编译期SQL校验。
默认中文、结论先行；构建/Mock不等于真实DB、RLS角色、外部HTTP、浏览器/设备、容量、灾备或生产部署验收。未执行和失败必须明确，历史全绿不继承为当前结论。
