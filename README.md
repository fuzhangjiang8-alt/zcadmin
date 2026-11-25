# 卓才智能

Rust（Axum + sqlx）与 Vue 3 管理平台，采用模块化单体和编译期插件。PostgreSQL 是主存储，内核提供身份、权限、租户、审计、文件及公共运行能力。

## 快速开始

环境：Rust 1.85+、PostgreSQL 16+、Node 22 较新版本或 24。Windows GNU Rust 需将已安装的 MinGW bin 加入 PATH；前端测试使用 Node 类型擦除与 registerHooks。

```bash
# 根目录；已有 PostgreSQL 时可跳过 compose，用 DATABASE_URL 指定开发库
docker compose up -d postgres
cargo run -p server

# 另开终端，在 frontend/ 中运行
cd frontend
npm ci
npm run dev
```

前端默认 `http://localhost:5173`，后端 `http://127.0.0.1:8080`。检查 `/healthz` 和 `/readyz`。
空库初始化账号来自 `config/default.toml`，仅供开发；生产通过 `AUTH_DEV_PASSWORD` 提供强口令，已有账号密码不会被覆盖。
Windows 也可用 `powershell -ExecutionPolicy Bypass -File scripts/dev.ps1` 启动后端。

## 从哪里开始读

| 需求 | 文档 |
|---|---|
| 全局边界、NFR、决策 | [架构总览](docs/企业级管理后台-架构设计.md) |
| 找代码、确认已实现/未实现 | [后端代码地图](docs/10-后端代码地图.md) |
| 修改后端 / 前端 / 数据 | [01 后端](docs/01-后端架构.md) / [02 前端](docs/02-前端架构.md) / [03 数据](docs/03-数据架构.md) |
| 开发插件 | [04 插件](docs/04-插件体系.md)及目标插件 CONTEXT.md |
| 部署、完整环境变量、故障处理 | [05 安全运维](docs/05-安全合规与运维.md) |
| 日常贡献、注释要求 | [07 开发流程](docs/07-AI原生开发与持续迭代.md) / [09 注释规范](docs/09-注释与文档规范.md) / [AGENTS.md](AGENTS.md) |
| 多产品 / 桌面与移动 | [06 平台演进](docs/06-平台化演进（中台+多产品）.md) / [08 多端](docs/08-多端架构（Web+Tauri+Flutter）.md) |
| 本轮基础改动 / 已执行验证 | [基础报告](docs/基础功能分析与加固-2026-11-10.md) / [验收记录](docs/ci-local-rehearsal.md) |
| Rust 入门 / 旧版详细资料 | [代码导览](RUST-新手导览.md) / [历史归档](_archive/docs/2026-11-10-before-simplify/INDEX.md) |

## 代码组织

| 目录 | 职责 |
|---|---|
| `crates/kernel/` | 内核及 `migrations/`、`tests/` |
| `crates/xtask/` | 开发与验收命令 |
| `apps/server/` | 装配入口、全链路集成测试 |
| `plugins/*` | 各业务域实现和独立迁移 |
| `frontend/src/` | 页面、features、请求客户端、共享查询缓存与前端插件 |
| `config/`、`contracts/`、`ops/` | 配置、生成契约、运维脚本 |

## 开发与验收

```bash
cargo xtask verify       # fmt、clippy、普通测试、依赖边界、RLS 静态、权限/错误码
cargo xtask check-perms  # 单独检查跨端权限及错误码
cargo xtask openapi      # 更新 contracts/openapi.json

# frontend/ 内
npm run gen:api          # OpenAPI → TS，禁止手改生成物
npm test
npm run build           # 类型检查 + Vite 构建
```

`verify` 不包含前端构建、契约重新生成和 ignored 真库测试。真库/RLS 角色验证的完整命令只维护在[开发流程](docs/07-AI原生开发与持续迭代.md)，执行结果只维护在[验收记录](docs/ci-local-rehearsal.md)。

## 使用边界

- 会话撤销只清本实例缓存，其他实例最长有 30 秒窗口；登录限流也按实例计数。
- 文件下载仍全量读内存；物理删除失败需运维清理。用户同步导入最多 500 行。
- MFA、Refresh 轮换、持久化任务调度、Outbox 等见代码地图中的待实现项。
- 构建、静态门禁和 Router 测试不代表浏览器、生产角色隔离、容量或部署验收完成。
- 生产数据库禁止使用超级用户或 BYPASSRLS 角色；已有数据卷按 `ops/init-app-role.sql` 核对角色和授权，不以删除数据卷代替升级。
