# 验收记录

只记录执行时间、范围、结果与未测项；命令统一见[07开发流程](07-AI原生开发与持续迭代.md)，基础改动见[完善记录](基础功能分析与加固-2026-11-10.md)。历史全绿不继承为当前状态。

## 0. 2026-11-10 AGENTS源码对照（当前）

对照workspace、xtask/生成器、后端身份/事务及前端请求/缓存重写AGENTS，保留此前版本于_archive/agents；同步07中的实际行数门禁说明，未修改实现代码或插件。
AGENTS的9个本地链接/锚点、18个实现入口、7个xtask命令及3个npm脚本已核对。
本次 `cargo xtask verify` 六步全部通过：44项内核单测、3项HTTP契约；架构/300行门禁、42张表RLS静态及权限/错误码一致性均通过。
ignored真库、前端构建、浏览器/部署未在本次重跑。以下失败属于各次历史快照，不覆盖本次结果。

## 1. 2026-11-10 文档精简

范围：README、Rust导览及docs主文档。旧版15份已归档并有SHA-256清单，ADR、AGENTS、规格及插件CONTEXT保留。
本次只改文档与归档，不修改实现、插件、迁移、配置或生成契约。

| 检查 | 本次结果 |
|---|---|
| 文档结构 | 15份主文档精简；部署变量集中05，命令集中07，结果集中本页 |
| 链接/锚点/编码/代码块 | 32份现行及引用文档、83个本地链接检查通过 |
| 配置覆盖 | Config解析的28个环境变量在05中均有记录 |
| 原文归档 | 15份SHA-256核对一致，清单和恢复说明见归档INDEX |
| cargo xtask verify | fmt阶段失败：system_depts.rs、system_roles.rs、system_users_create.rs存在格式差异 |

格式检查失败后未执行后续Clippy/测试/门禁；本次未重跑真库、浏览器、前端构建和部署验证，也未改动上述代码来消除差异。

## 2. 2026-11-10 上次文档同步

| 检查 | 当时结果 |
|---|---|
| Markdown | 8份、23个本地链接/编码/代码块通过 |
| cargo xtask verify | fmt阶段失败：kernel/src/ssrf.rs和plugins/crm/src/perm.rs格式差异 |
| 后续阶段 | 因首步退出未执行；未重跑真库、前端构建、生产验证 |

保留当时状态，不暗示当前仍有相同故障，也不以更早通过覆盖失败。

## 3. 2026-11-10 基础代码完善

环境：Windows GNU Rust、本机PostgreSQL；sqlx测试自动创建独立库，不复用业务账号。以下是该轮真实执行结果：

| 检查 | 结果/边界 |
|---|---|
| verify | 六步通过；43项内核单测、3项HTTP契约；ignored真库需另跑 |
| session_atomicity | 3项通过：8路并发、失败回滚、多端/租户边界 |
| file_atomicity | 3项通过：提交失败保留内容、正常删除、权限拒绝 |
| user_import_atomicity | 2项通过：异常行隔离、dryRun不新增用户 |
| OpenAPI/TS | 正式生成通过；退出契约204/503 |
| 前端 | 23项Node测试、vue-tsc/Vite构建通过，无浏览器实点结论 |
| 静态门禁 | 68权限/9错误码一致，42张有策略表均FORCE；不等于实际库已迁移/角色隔离通过 |

历史错误码demo.item.not_found保留为兼容性提示，不是失败。数字为该次快照，包含当时并行变更，不表示均由本轮新增。

## 4. 更早CI演练与未验证项

原记录环境为Windows 10、PostgreSQL 18、Node 22；曾运行smoke、RBAC/范围、会话、安全基线、导出维护等，具体日志与排查保存在[原记录](../_archive/docs/2026-11-10-before-simplify/docs/ci-local-rehearsal.md)。本次未将它们全部重跑。
Webhook delivery曾因本机回环连接超时未通过；历史exe/便携包不代表当前安装包生成成功。
Docker/compose交付、Flutter/设备、Tauri安装包、Linux SIGTERM/滚动发布、真实网关容量、生产RLS角色和灾备恢复均须按版本独立验收。
