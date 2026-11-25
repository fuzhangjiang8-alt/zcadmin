# Rust代码阅读导览

先按[README](README.md)启动项目；不熟悉模块时查[代码地图](docs/10-后端代码地图.md)，完整规则和命令不在本页重复。

## 1. 先读这五处

| 顺序 | 文件 | 理解什么 |
|---|---|---|
| 1 | apps/server/src/main.rs | 程序入口、配置和监听/停机 |
| 2 | crates/kernel/src/app.rs | 连接池、迁移、插件和路由如何装配 |
| 3 | crates/kernel/src/state.rs | 请求共享什么依赖，为什么Clone便宜 |
| 4 | crates/kernel/src/auth.rs、auth_login.rs | 提取身份、参数、错误及会话 |
| 5 | plugins/demo/src/lib.rs | 一个真实插件怎样声明路由/权限/迁移 |

示例以当前源码为准，不照抄归档文档中未实现的trait、宏或状态机API。

## 2. 一次请求怎样走

```text
路由和中间件 → 提取参数/身份 → 权限校验 → 租户事务
→ 业务/数据库操作 → 提交 → 响应
```

State<AppState>提供依赖，CurrentUser提供身份，TenantTx给连接注入事务级租户上下文。功能权限不等于数据权限；RLS也不替代接口鉴权。
若多个提取器都解析身份，先看真实调用链，不把Rust参数顺序理解成Java对象注入容器。

## 3. 常见语法和边界

| 写法 | 在本项目中的含义 |
|---|---|
| Result<T, E>、? | 成功或明确失败；?向上传播错误，不能吞错假成功 |
| Option<T> | 可能没有值，匹配处理；非测试禁止unwrap/expect |
| async fn、await | 允许I/O等待让出执行；不代表同步CPU任务自动不阻塞 |
| Arc、Clone | 共享持有依赖，不复制整份数据库；全局可变状态仍需明确并发规则 |
| trait、impl | 定义与实现契约，Plugin是实际例子 |
| pub、pub(crate)、mod | 对外/本crate可见及模块边界，公开项写文档注释 |
| ownership/借用 | 谁拥有值、是否能共享/修改；提交可能消费事务，不能提交后继续使用 |

SQL值用bind，身份/范围由可信来源传入。Excel/Argon2走受控池，不直接放在async worker里。

## 4. 第一次改动怎么做

1. 只跟一条路由，查DTO、权限常量、表/迁移、调用方和测试。
2. 阅读目标CONTEXT与01分层，写明验收和失败路径。
3. 小步修改并检查类型/借用错误，保留清晰中文业务约束注释。
4. 按[开发流程](docs/07-AI原生开发与持续迭代.md)执行针对回归和verify；真库、浏览器与部署分别确认。

完整Rust概念教程与逐行讲解保留在[精简前原文](_archive/docs/2026-11-10-before-simplify/RUST-新手导览.md)，日常定位优先看源码和10。
