# zcadmin 代码安全复检（增量，2026-11-14）

## 0. 方法与边界

| 项 | 说明 |
|---|---|
| 方法 | 只读静态审计：逐文件读源码 + 正则全仓扫描；未修改任何项目代码/配置/数据 |
| 实际执行 | `target\debug\xtask.exe check-perms`（通过）、`target\debug\xtask.exe arch-check`（通过） |
| 未执行 | 未连数据库、未启动服务、未跑 `apps/server/tests/*`（需要专用测试库），因此所有结论均为**静态证实**，标注「需运行验证」的条目未实测 |
| 仓库状态 | 本目录不是 git 仓库（`git status` → `fatal: not a git repository`），无法给出 diff 口径，结论基于当前工作区文件 |
| 与旧审计的关系 | 本文件是 **增量**。基线见 [`backend-security.md`](backend-security.md)，那份额外审计对应的是拆分前的旧代码（`system.rs` 单文件、`auth.rs` 1300+ 行、无安全头、无 CSRF）。本次逐条复核了它的 P0–P2，**已修复的见第 3 节**，不再重复报告 |

风险分级：P0 可直接导致失陷/清空数据；P1 越权或绕过关键控制；P2 加固缺口/信息泄漏；P3 一致性与代码质量。

---

## 1. 结论摘要

1. **没有发现 SQL 注入、路径穿越、命令执行、反序列化、`unsafe`**。动态 SQL 全部来自内部常量（`scope::condition` 的 alias、`audit::snapshot` 的 table、`tenant_purge` 的目录读数），用户输入一律走绑定参数；存储 key 由服务端生成且 `resolve` 拒绝 `..`/绝对路径（`storage.rs:36-42`）。
2. **最需要立刻改的是 1 处已确认的横向越权**：`system.users` 异步导出完全不走数据范围过滤，与列表接口语义不一致，等于给「self 范围」用户开了全租户用户/邮箱导出通道（P1-1）。
3. **SSRF 防护存在一条绕过链**：审计配置包导入直接落库 webhook URL（无 `validate_webhook_url`），而用户级订阅投递在发送前也不复检目标地址（P1-2）。加上 `ssrf.rs` 只做字面 IP/主机名判断、不做 DNS 解析，域名指向内网亦可绕过。
4. **PAT（个人访问令牌）脱离凭据轮换与租户生命周期**：管理员重置密码、停用租户都只吊销会话，不动 `sys_api_token`（P1-3）。这是应急响应会直接失效的点。
5. 「首登强制改密」的判定依据是**环境变量是否设置**，而不是密码是否为默认值/是否弱口令；配一个合规但猜得到的口令即可绕过（P1-4）。
6. 其余为加固项：`/metrics` 默认开放、告警订阅 LIKE 通配可读全租户告警、导出把内部错误文本回传客户端、平台级能力仅靠权限点（依赖迁移里的 `scope='platform'` 前缀约定）、上传的内存放大、导出无并发上限。

计数：P1 × 4、P2 × 6、P3 × 6（P0 = 0）。

---

## 2. 发现清单

### P1-1 用户列表导出绕过数据权限（横向越权）〔已静态证实；建议补运行验证〕

**位置**

| 事实 | 证据 |
|---|---|
| 任务创建只校验权限点，不记录/校验范围语义 | `crates/kernel/src/exports_api.rs:134-144` |
| `data_scope` / `dept_id` 被塞进 `ExportJob`，但 `system.users` 分支丢弃 | `crates/kernel/src/exports_runner.rs:77` → `export_users(&mut tx, user_id)` |
| 导出 SQL 无任何范围条件、无 `tenant_id` 以外的行级约束 | `crates/kernel/src/exports.rs:204-215` |
| 同权限的列表接口是**有**范围过滤的 | `crates/kernel/src/system_users.rs:112`（`scope::condition_with_owner`） |

**影响**：任何拥有 `system:user:list` 的账号，即使数据范围是 `self`、部门也看不到别人，也能通过 `POST /api/system/export-tasks {kind:"system.users"}` + `GET /{id}/download` 拿到**全租户**用户的 username / display_name / email / 部门名 / 状态（最多 20000 行）。邮箱虽按 `system:user:view_sensitive` 脱敏（`exports.rs:184-202`），但用户清单本身已是范围外数据。

**修复**：把 `ExportContext` 里的 `data_scope`/`dept_id` 真正用起来，与 `list_users` 复用同一段 SQL 片段：

```rust
// exports.rs::export_users 增加 ctx 参数
let scope = crate::scope::condition_with_owner("u.id", "u.dept_id", "$2", "$3", "$4");
// WHERE u.deleted_at IS NULL {scope}  → bind(ctx.data_scope), bind(ctx.user_id), bind(ctx.dept_id)
```

**验证**：新增集成测试——建一个 `data_scope=self` 的用户 A，用 A 建导出任务，断言 xlsx 行数 == A 自己的可见行数（当前会等于全租户行数）。

---

### P1-2 审计配置导入绕过 SSRF 校验，投递路径也不复检（盲 SSRF）〔已静态证实〕

**位置 / 对照**

| 路径 | 校验 | 证据 |
|---|---|---|
| 保存租户级 Webhook | ✅ 校验 | `audit_alert_settings.rs:197` → `validate_webhook_url` |
| 保存用户级订阅 Webhook | ✅ 校验 | `audit_subscriptions.rs:262-264` |
| 发送租户级 Webhook 前 | ✅ 复检 | `audit_notify.rs:118` |
| **导入配置包写两张 URL 表** | ❌ 无校验 | `audit_config_import.rs:217-229`（订阅）、`232-249`（租户级） |
| **发送用户级订阅前** | ❌ 不复检 | `audit_subscription_dispatch.rs:117-130` 直接 `client.post(&url)` |

**攻击链**（需要 `system:audit:alert`，租户管理员默认拥有）：
1. `PUT /api/system/audit-config/import?mode=import`，body 里带
   `{"subscriptionPatterns":["*"],"subscriptionWebhookUrl":"http://169.254.169.254/latest/meta-data/","subscriptionWebhookEnabled":true, ...}`
2. 后台 `dispatch_subscriptions` 每轮把租户内告警 POST 到该地址；
3. 目标不可达/超时/4xx 会写进 `sys_audit_alert_delivery.last_error` 与 `last_error_kind`，配合 `GET /api/system/audit-deliveries` 可区分「网络可达但 404」与「连接被拒」——即**内网端口/路径探测**。

`audit_notify.rs:92-96` 与 `audit_subscription_dispatch.rs:39-42` 都已 `redirect(Policy::none())` + 5s 超时，缓解了「302 跳内网」；但**目标地址本身的校验缺口仍在**。

**次生问题（同一模块）**：`ssrf.rs:45-52` 只对 `host.parse::<IpAddr>()` 成功的字面量做私网判断，**不做 DNS 解析**。`http://attacker.example.com`（A 记录 → `10.0.0.5`）或 DNS rebinding 都能通过。`is_private_or_loopback_ip` 也未覆盖 `100.64.0.0/10`（CGNAT）、`198.18.0.0/15`、`::ffff:127.0.0.1` 映射等。

**修复**
1. `audit_config_import.rs` 在落库前对 `subscription_webhook_url` / `webhook_url`（非空且 enabled 时）调用 `crate::ssrf::validate_webhook_url`；
2. `dispatch_subscriptions` 在发送前对 `d.url` 复检（与 `notify_pending` 对齐）；
3. `ssrf` 增加「解析后校验全部 A/AAAA 记录」+ 请求时固定已解析 IP（或走带 `resolve` 钩子的自建 client），并补齐 CGNAT/保留段。

**验证**：`cargo test -p server --test smoke` 系列里有 webhook 用例（`apps/server/tests/webhook_retry.rs`），可照此加一条「导入的私网 URL 必须被拒绝或不被投递」的断言。

---

### P1-3 PAT 生命周期脱离凭据轮换与租户状态〔已静态证实〕

| 场景 | 现状 | 证据 |
|---|---|---|
| 管理员重置用户密码 | 只 `revoke_user_sessions` | `system_security.rs:262-265` |
| 用户自助改密 | 仅当 `revokeTokens=true` 才吊销 PAT | `auth_session_api.rs:192-201` |
| 停用/删除租户 | 只吊销该租户会话 | `tenant_lifecycle.rs:72-74`、`192` |
| PAT 解析 | 校验 `user.status` / `expires_at` / `revoked_at`，**不校验租户 status** | `auth.rs:169-188` |
| PAT 默认有效期 | `expires_in_days=None` → 永不过期 | `auth_tokens.rs:113-119` |
| 单端登录 | 对 PAT 无影响（PAT 路径 `session_id: None`） | `auth.rs:225` |

**影响**：① 账号被盗后重置密码，攻击者手里的 PAT 仍长期有效 → 事件响应失败；② 租户被 `suspended`/`deleted` 后，其 PAT 仍能通过 `TenantTx::begin_tenant` 访问该租户数据（登录接口会把非 active 租户挡在 `resolve_tenant`，PAT 没有等价检查）。

**修复**：`reset_user_password` / `update_user_status(disabled)` / `revoke_tenant_sessions` 同事务内 `UPDATE sys.sys_api_token SET revoked_at = now() WHERE tenant_id=$1 AND (user_id=$2 / TRUE)`；`resolve_api_token` 增加 `JOIN sys.sys_tenant` 并要求 `status='active' AND deleted_at IS NULL`；创建 PAT 时给默认有效期（例如 90 天）。

---

### P1-4 「强制首登改密」按环境变量是否设置判定，弱口令可用合规密码绕过〔已静态证实〕

```rust
// crates/kernel/src/system_seed.rs:26-29
fn initial_admin_must_change() -> bool {
    std::env::var("AUTH_DEV_PASSWORD").is_err()   // 只要设了 env 就不强制改密
}
```

- `docker-compose.yml:76` 固定注入 `AUTH_DEV_PASSWORD: ${AUTH_DEV_PASSWORD:-admin123456}`，因此容器场景 `must_change_password` **恒为 false**。
- release 构建在 `system_seed.rs:43-49` 用默认策略（`min_length=10` + 大写 + 小写 + 数字）校验初始口令：`admin123456` 无大写 → 启动失败（fail-closed，**这是好事**，但意味着 `docker compose up` 在空库上起不来，部署文档需明说）。
- 真正的口子：只要口令满足策略即可，例如 `Admin123456`（正是 `password.rs:223` 单测里用的例子）——release 正常启动、`must_change_password=false`、口令可猜。**首登改密这道控制等于没有。**

**修复**：把判定从「env 是否设置」改成「口令是否等于默认值 / 是否命中弱口令表」，例如新增 `AUTH_FORCE_PASSWORD_CHANGE=true|false` 显式开关，默认 `true`（`ensure_seed` 里 `must_change = force || password_is_default`）；compose 去掉 `:-admin123456` 默认值，改为必填。

---

### P2-1 `/metrics` 默认对任何可达者开放〔已静态证实；部署事实相关〕

- 路由无条件注册：`app.rs:98-101`；处理器只有 `METRICS_TOKEN` 存在时才校验（`metrics.rs:192-201`，常量时间比较实现正确）。
- 暴露：路由模板、各状态码计数、P50/P99 延迟、`db_pool_connections/idle/saturated`、版本号——足以做攻击面测绘与容量画像。
- `docs/05-安全合规与运维.md:41,90` 已把「外部可达时设置 `METRICS_TOKEN` 或网络隔离」写成运维要求，但**默认值是开放的**，等于把安全责任完全推给部署。
- **修复**：默认要求 token（未设置则 404/401），或改成默认只监听内网/仅 `/readyz` 暴露。启动时若未设 token 打 `WARN`。

---

### P2-2 告警订阅模式未转义，任意登录用户可读全租户告警〔已静态证实〕

```sql
-- audit_subscriptions.rs:149-153（列表）、audit_subscription_dispatch.rs:63（投递）
AND a.action LIKE replace(s.action_pattern, '*', '%')
```

- 订阅写入只校验「非空、≤128 字符」（`audit_subscriptions.rs:87-94`），`%`、`_` 原样入库；
- 用户把 pattern 设为 `%` → 匹配全部 action；
- `GET /api/system/audit-alerts/subscribed` 只要求登录（无权限点，`audit_subscriptions.rs:133-138`），返回 `action/window/count/threshold/user_id/username`。

**影响**：租户内任意登录用户（哪怕零权限）可读到全租户告警，包括 `auth.login.failed` 的具体用户名、配额/配置变更、`system.delivery.dead` 等运维事件。

**修复**：入库前转义（`replace('%','\%')` 并配 `LIKE ... ESCAPE '\'`），或改成「只允许前后缀 `*` 的通配」并显式编译成 `prefix%` 形式；同时给该接口加一个低成本权限点。

---

### P2-3 导出/I 导入失败原因原文回传客户端〔已静态证实〕

| 位置 | 内容 |
|---|---|
| `exports_runner.rs:195` | `mark_failed(..., &e.to_string())`，anyhow 链含 SQL/文件路径 |
| `exports_api.rs:96` | 列表接口把 `error` 字段原样返回 |
| `imports.rs:109-111` | 插件 `anyhow::Error` → `AppError::Validation(e.to_string())`，直接进 problem+json 的 `detail` |

与 `error.rs:108-126`「5xx 详情只入日志、对外固定文案」的设计目标冲突（这两条路走的是 2xx/422 通道，绕过了该收敛点）。

**修复**：落库/回显用稳定分类（`read_only_tx`、`unknown_kind`、`xlsx_invalid` 等，`audit_subscription_dispatch.rs:12-32` 的 `classify_error` 是现成范式），原始 `e.to_string()` 只进 `tracing::error!`。

---

### P2-4 平台级能力只靠权限点，`scope` 默认值是 `tenant`（默认放行式脆弱点）

- 平台级 handler 只做 `require_perm(TENANT_*)`，没有 `is_super_admin()` 断言：`tenant_lifecycle.rs:52,113,177,219`、`tenant_admin.rs:97`、`tenant_purge_api.rs:74`、`tenant_bill_payments.rs` 等。
- 闭合靠两处约定：迁移 `0067_platform_permission_scope.sql` 把 `code LIKE 'system:tenant:%'` 标成 `platform`；`system_role_permissions.rs:128-145` 拒绝非超管授予 `scope<>'tenant'` 的权限点。
- 但 `sys_permission.scope` 的**列默认值是 `'tenant'`**（0067 第一句）。也就是说：**新增任何平台级权限点，只要编码不带 `system:tenant:` 前缀，就默认落到「租户管理员可自授」**——从默认拒绝变成默认放行，且不会有任何测试报警。

**建议**：① 平台级 handler 内同时断言 `user.is_super_admin()`（防御性，成本一行）；② 迁移把默认值改 `'platform'`，租户级权限点在声明处显式标 `tenant`；③ 在 `xtask check-perms` 里加一条：`plugin='system'` 且 `code` 前缀为 `system:tenant:` 之外的新增权限需人工确认 scope。

---

### P2-5 上传/导入的内存放大路径

| 事实 | 证据 |
|---|---|
| 上传路由放开到 25MB，且 `field.bytes()` 全量读进内存 | `files.rs:70`、`files.rs:159-162` |
| 全局并发上限默认 `CPU×8`（32 核 = 256） | `app.rs:126`、`252-260`、`153` |
| 其它接口默认 2MB | `app.rs:201` |
| CRM 导入单请求同步处理最多 5000 行，无独立 body 限制（继承 2MB） | `plugins/crm/src/excel.rs:39`、`excel_import.rs:76` |

理论上 `25MB × 256 ≈ 6.4GB` 峰值（同一租户有 `system:file:upload` 的账号即可）。有 `MAX_IMPORT_ROWS` 与配额检查（`files.rs:176-194`）兜住业务量，但**没有全局内存/在途字节预算**。

**建议**：上传路由单独挂一个更小的并发闸（`ConcurrencyLimitLayer`）或用 `axum::body::Body` 流式落盘（`tokio::fs` + 分块写），而非 `field.bytes()`。

---

### P2-6 导出任务无并发/配额限制，且用 `tokio::spawn` 直接起

`exports_api.rs:175-197`：每个请求 `INSERT` 一条任务后立刻 `tokio::spawn(run_export)`。没有「同用户在途任务数」「每租户 QPS」限制，`run_export`（`exports_runner.rs:45-99`）在**持有事务**的情况下查最多 20000 行并在事务内 `storage.save`（`exports_runner.rs:82-83`），xlsx 本身走了 `xlsx::run_anyhow` 阻塞池但事务未释放。

**影响**：有导出权限的账号可发起 N 个并发导出 → 连接池被长事务占满（`statement_timeout` 管不到「持有事务等待 CPU」的时段）→ 503；同时 `storage.save` 在事务内，违反 AGENTS「禁止事务内等外部 IO」的精神。

**建议**：把 `storage.save` 与 `UPDATE ... file_key` 移到事务外（先查询生成字节 → 提交 → 落盘 → 独立事务更新状态）；对 `sys_export_task` 增加「同用户 pending+running ≤ N」的准入检查。

---

### P3 级

| # | 问题 | 证据 | 建议 |
|---|---|---|---|
| P3-1 | `/api/demo/ping` 无 `CurrentUser`，demo 插件启用即公开 | `plugins/demo/src/lib.rs:91-99` | 加 `_user: CurrentUser` 或从生产禁用 `demo`（`DISABLED_PLUGINS`） |
| P3-2 | CSRF 同源判定忽略 scheme，且优先信任 `X-Forwarded-Host` | `csrf.rs:51-68`（`same_origin` 只比 authority） | 比较 `scheme://host:port` 全量；`X-Forwarded-Host` 仅在可信代理下采信（可复用 `state.trusted_proxies`） |
| P3-3 | 未传租户编码时，多租户库返回「系统存在多个租户」 | `auth_identity.rs:49-55` | 与 `code` 分支保持一致，统一 401 或 422 且文案不透露租户数量 |
| P3-4 | `TenantTx` 的 `Deref/DerefMut` 用 `expect` 直接 panic | `tenant.rs:105-117` | 改成返回 `Result` 的访问器；至少 `debug_assert` + 显式错误（有 `CatchPanic` 兜底但仍是 500） |
| P3-5 | `generate_password()` 是死代码，且只产出小写十六进制（永不满足「含大写」的默认策略） | `password.rs:102-110`（全仓 0 处调用） | 删除，或改为 `rand`+全字符集并补单测 |
| P3-6 | `sys_tenant` / `sys_permission` / `sys_user_role` / `sys_role_permission` / `sys_session` / `sys_user_post` 无 RLS（实测迁移：`0001`/`0002`/`0020`/`0025` 无 `ENABLE ROW LEVEL SECURITY`） | `system_identity.rs:79-102`、`posts_user.rs:147` 已用显式 `tenant_id` 校验兜底 | 现状可接受（关联表无 `tenant_id`、`sys_tenant` 本身即租户根），但需在 `docs/03` 明确「这些表禁止任何按 id 的单条件写法」；`rls_enforced` 测试应把它们列为已知例外 |

---

## 3. 旧审计（`.audit/backend-security.md`）中已修复、不应再报的项

复核旧文档 P0–P2 后，以下问题在当前代码里**已闭环**（列出以免重复投入）：

| 旧问题 | 当前状态 | 证据 |
|---|---|---|
| 无安全响应头（CSP/HSTS/XFO/nosniff/Referrer-Policy） | 已加全套 + Permissions-Policy | `security.rs:25-71` |
| 禁用/锁定用户后会话仍有效 8 小时 | 已吊销全部会话 | `system_users.rs:228-240` |
| 角色/权限变更不即时生效 | 撤销持有该角色的会话 | `system_role_permissions.rs:172-183` |
| 租户管理员可自授 `system:tenant:*`（is_super 越权） | 已加 `scope='platform'` + 授权校验 + 历史数据清理 | `0067_platform_permission_scope.sql`、`system_role_permissions.rs:128-145` |
| CSRF 无任何防护 | Origin + Sec-Fetch-Site 双层校验 | `csrf.rs:25-60` |
| 登录失败可枚举账号（时序） | `burn_verification_time` 对齐 Argon2 成本 | `password.rs:89-100`、`auth_login.rs:89-97` |
| 直连客户端可伪造 `X-Forwarded-For` | 可信代理白名单 + 从右往左取第一个不可信跳 | `client_ip.rs:150-193` |
| `/docs`、`/api/openapi.json` 无条件暴露 | 由 `EXPOSE_API_DOCS` 控制，默认 true 但可关 | `app.rs:103-116` |
| CRM `ensure_customer_visible` 等只靠 RLS | 已全部经 `scope_filter` 别名过滤 | `plugins/crm/src/api.rs:37-94` |
| 错误模型 5xx 泄漏 SQL | 已固定文案 + 只记日志 | `error.rs:111-126` |

---

## 4. 需要运行环境才能确认的项（未做）

1. **RLS 真实隔离**：本次只静态比对迁移，未以 `NOSUPERUSER NOBYPASSRLS` 角色跑 `apps/server/tests/rls_enforced.rs`。静态门禁（`xtask verify` 里的 RLS 覆盖检查）不能替代角色隔离验证。
2. **P1-1 的实测**：需要一个 `data_scope=self` 的账号 + 专用测试库，跑一次导出并核对行数。
3. **P1-2 的实测**：需要能构造出网请求的环境，验证「导入私网 URL → 是否产生投递记录/`last_error`」。
4. **P2-5 的峰值内存**：需要压测（当前没有该口径的测试）。
5. 前端：本次只核对了 `api/client.ts`（`credentials:'include'`、不存 token、无 `localStorage` 令牌）与全仓 `v-html`/`innerHTML`/`eval` 0 命中；组件级 XSS、依赖供应链（`npm audit`）未做。

---

## 5. 最小改动建议（按性价比排序）

1. `exports.rs::export_users` 接 `ExportContext` 加 `scope::condition_with_owner`（1 个文件，~10 行）——直接关掉 P1-1。
2. `audit_config_import.rs` 两处 URL 落库前 `validate_webhook_url` + `dispatch_subscriptions` 发送前复检（~8 行）——关掉 P1-2 主链。
3. `reset_user_password` / `update_user_status` / 租户停用 三处补 PAT 吊销；`resolve_api_token` 加租户 status（~15 行）——关掉 P1-3。
4. `/metrics` 默认要求 token（~5 行）——关掉 P2-1。
5. 订阅模式转义（~3 行）——关掉 P2-2。
6. 导出/导入错误改稳定分类码（~10 行）——关掉 P2-3。

改完按 `AGENTS.md` 第 7 节跑 `cargo xtask verify`；涉及接口/权限点变更再跑 `check-perms` / `openapi` / 前端 `npm run gen:api && npm test && npm run build`。

---

## 6. 修复记录（2026-11-14，同日落地）

### 6.1 逐条状态

| 编号 | 修复内容 | 主要改动 | 验证 |
|---|---|---|---|
| P1-1 | 用户导出按数据范围过滤 | `exports.rs::export_users` 改接 `ExportContext`，拼 `scope::condition_with_owner("u.id","u.dept_id")`；`exports_runner.rs` 提前构造 ctx | **运行验证**：`scope_matrix` 新增回归（self 范围用户导出 `totalRows == 1`），真库通过 |
| P1-2 | SSRF 三处收紧 | ①`audit_config_import` 落库前校验两条 URL；②`audit_subscription_dispatch` 发送前复检，命中拒绝直接置死信（`ssrf_blocked`，不再退避）；③新增 `ssrf::validate_webhook_url_async`（字面量 + 真实 DNS 解析结果逐个校验），四个保存/发送点统一改用它；④保留段补齐 CGNAT/文档段/240-4 与 IPv4 映射地址 | 单测（`blocks_cgnat_and_mapped_addresses`、异步档、`split_host_port`）+ `smoke` 全过 |
| P1-3 | PAT 纳入凭据轮换与租户生命周期 | `auth_tokens` 新增 `revoke_user_tokens`/`revoke_tenant_tokens`；重置密码、停用/锁定用户、停用/删除租户均调用；`resolve_api_token` 增加 `sys_tenant` 状态与 `deleted_at` 条件 | **运行验证**：`rbac_role` 新增回归——管理员重置密码后，此前签发的 PAT 与该用户会话均返回 401 |
| P1-4 | 强制首登改密不再只看环境变量 | `system_seed::initial_admin_must_change`：`AUTH_FORCE_PASSWORD_CHANGE` 显式覆盖优先；**release 构建一律强制**；debug 保持旧口径（避免本地/CI 反复改密）；未知取值打 warn 后按默认 | 逻辑变更，release 行为需部署验证（见 6.3） |
| P2-1 | `/metrics` 默认关闭远程访问 | 抽出 `metrics_export.rs`：有 `METRICS_TOKEN` 走常量时间鉴权（401），无令牌只允许环回/无对端（进程内），其余 404；启动时 WARN | `security_baseline::metrics_requires_token_when_configured` 通过；新增访问策略单测 |
| P2-2 | 订阅 LIKE 通配转义 | 新增 `audit_subscriptions::like_expression`，列表/计数/投递三处改成 `... ESCAPE '\'`，只把 `*` 当通配 | 编译期 + `smoke` 通过；转义表达式有单测锚定 |
| P2-3 | 内部错误不再回传客户端 | 导出失败落库改稳定文案（原始 `{e:#}` 只进日志）；`imports.rs` 记录完整错误链；CRM 四个导入器不再把 sqlx 原文拼进「第 N 行」消息 | 编译期 + `smoke` 通过 |
| P2-4 | 平台能力双保险 + 权限默认拒绝 | 新增 `require_platform`，26 处 `system:tenant:*` handler 全部加身份断言；迁移 `0073` 把 `sys_permission.scope` 默认值改为 `platform`，`sync_plugin_permissions` 显式写 `tenant` | `smoke`（租户创建/停用/账单）、`rbac_matrix` 通过 |
| P2-5 | 上传并发收口 | `files.rs` 加进程内许可池（4 个），handler 开头 `try_acquire`，满即 503，先占许可再读 25MB 请求体 | `smoke` 上传流程通过 |
| P2-6 | 导出不再事务内落盘 + 在途上限 | `run_export` 拆成「事务A 生成 → 事务外落盘 → 事务B 写结果」；`create_task` 增加同用户 `pending+running ≤ 5` 的准入（超出 429） | `smoke` 导出全流程通过 |
| P3-1 | demo `/ping` 需登录 | `plugins/demo/src/lib.rs` 加 `_user: CurrentUser` | 编译期 |
| P3-2 | CSRF 校验协议 | 有 `X-Forwarded-Proto` 时比较 `scheme + authority`，无则退回只比 authority；新增降级用例 | 单测（`same_origin_rejects_scheme_downgrade`） |
| P3-3 | 租户解析文案 | 多租户时只返回「请填写租户编码」，不再暴露租户数量 | 编译期 |
| P3-5 | 删除死代码 | 移除 `password::generate_password`（全仓 0 调用，且产出永不满足含大写的默认策略） | 编译期 |

### 6.2 顺带完成的结构调整

- **拆分超 300 行的文件**（`xtask arch-check` R3）：新增 `metrics_export.rs`（Prometheus 端点与鉴权）、`exports_download.rs`（导出下载）、`system_password_policy.rs`（密码策略读写）；`audit_alerts.rs` 收编「已订阅告警」查询，`like_expression` 留在订阅模块供两处复用。
- **消除重复实现**：导出下载改用 `files_read::content_disposition`（原先两处各写一遍 RFC 5987 编码）。
- **测试基础设施**：`scope_matrix` 原先用 `sys_tenant LIMIT 1` 取租户再要求 `admin` 存在其中，多租户库上必然失败（RowNotFound）；改为用 `/api/auth/me` 反查 admin 自己的租户，测试可在共享库重复执行。
- 文档同步：`docs/05` 更新 `/metrics` 默认证、`AUTH_FORCE_PASSWORD_CHANGE`、SSRF 两档校验与 PAT 吊销口径。

### 6.3 明确未闭环的部分

1. **PAT 默认永不过期**未改：`expiresInDays` 省略即不过期是已发布契约（前端与 OpenAPI 都这么写），改成默认有效期属于破坏性契约变更，需要产品决策 + 版本化，不在本次范围。
2. **SSRF 的 TOCTOU**：解析校验与实际建连之间仍有重绑定窗口，需要「按已校验 IP 建连」或出口 ACL，属部署侧措施。
3. **P3-4（`TenantTx` 的 `Deref` 用 `expect`）**未改：`Deref` 必须返回引用，去掉 panic 路径需要改 API 形态（返回 `Result` 的访问器）并波及所有 handler，收益低于改动面，保持现状 + 注释说明。
4. **P3-6（部分全局/关联表无 RLS）**未改：`sys_tenant`/`sys_permission`/`sys_user_role`/`sys_role_permission`/`sys_session`/`sys_user_post` 依赖显式 `tenant_id` 校验，改表结构风险大于收益，已记录为「禁止单条件按 id 写法」的约定。
5. **运行验证覆盖**：本次在本地真库执行了 `scope_matrix`（含新增导出数据范围回归）、`rbac_role`（含新增 PAT 吊销回归）、`security_baseline`、`csrf_guard`、`tenant_isolation`、`smoke`，全部通过；`cargo xtask verify`（fmt/clippy/单测/架构/RLS 静态/权限与错误码契约）全部通过。
   **未执行**：`rls_enforced`（需 NOBYPASSRLS 角色）、`webhook_retry`/`webhook_delivery`（需可控出网）、`single_session`、`crm_import_partial`、`export_maintenance`、`custom_scope`、`bench`。
   **仍需部署侧验证**：P1-4（release 构建的强制改密）、P1-2 的 DNS 解析在目标网络的真实行为、P2-5 的峰值内存。
