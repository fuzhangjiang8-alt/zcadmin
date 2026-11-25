# zcadmin 后端安全审计（只读）

- 仓库：`D:\fcj\zcadmin`
- 范围：`crates/kernel/src/*.rs`（31 个文件）、`plugins/crm/src/*.rs`、`plugins/demo/src/lib.rs`、`apps/server/src/main.rs`、`crates/kernel/migrations`（65 个）、`plugins/crm/migrations`（16 个）、`ops/*.sql`、`config/default.toml`、`docker-compose.yml`
- 方法：**只读**。用 `Select-String` / `grep` 全仓扫描，并用脚本解析全部 `.route(...)`（128 处注册，展开为 **176 个 method+path**），逐个 handler 检查 `require_perm` / `has_permission` / `perm::require`、`audit::record*`、`TenantTx`；同时逐条比对 65 个迁移中的 `CREATE TABLE` / `ENABLE|FORCE ROW LEVEL SECURITY`。未修改任何项目代码或配置。
- 口径：P0 = 可直接造成越权、跨租户数据泄漏、审计/会话控制失效，生产前必须修复；P1 = 高风险或系统性缺陷；P2 = 加固、一致性、文档/流程问题。

> **一句话结论**：写接口权限点覆盖了 **79/88**（9 个全是登录/登出/个人令牌/改密/通知已读/个人订阅这类自助接口），但**禁用用户不吊销会话、平台级权限与租户角色同域、`sys_user_role` 无 tenant_id/RLS、demo 插件整表无 tenant_id** 这四类问题会让"权限点齐全"在真实攻击面下失效；审计与业务不同事务且 27 个写路由无审计，使事后追溯不可依赖。

---

## 一、权限点执行覆盖率

### 结论

**128 个 `.route(...)` 注册展开为 176 个 method+path（GET 88、POST 45、PUT 21、DELETE 16、PATCH 6）；88 个写接口中 79 个在 handler 内显式调用权限校验，9 个没有权限点（均为认证/自助接口，但按 AGENTS 第 9 条字面要求仍属缺口）；权限点共声明 63 个、代码实际检查 61 个，0 个"检查但未声明"，2 个"声明但从不检查"的死权限点。**

### 证据表

| 项目 | 数字/结论 | 证据 |
|---|---|---|
| `.route(...)` 注册数 | 128 处 | `crates/kernel/src/system.rs:247-424`（70 处）、`crates/kernel/src/auth.rs:537-545`（6 处）、`plugins/crm/src/lib.rs:227-267`（16 处）、`plugins/crm/src/contact.rs:85-89`、`contract.rs:102-106`、`opportunity.rs:88-92`、`payment.rs:92-96`、`plugins/demo/src/lib.rs:128-133` |
| 展开后的 method+path | 176 | 解析 128 处 `.route(` 中链式 `get().post()` 等方法；统计见附录 A |
| 写接口总数 | 88 | POST 45 + PUT 21 + DELETE 16 + PATCH 6 |
| 有显式权限校验的写接口 | **79/88 = 89.8%** | 逐 handler 扫描 `require_perm` / `has_permission` / `perm::require`；完整矩阵见下表 |
| 无权限点的写接口 | **9** | 见下方"未受保护的写接口" |
| 权限点声明总数 | **63** | 内核 37 个：`crates/kernel/src/perms.rs:4-112` + 各迁移 `INSERT INTO sys_permission`；CRM 23 个：`plugins/crm/src/lib.rs:61-156`；demo 3 个：`plugins/demo/src/lib.rs:107-122` |
| 代码实际检查的权限点 | **61** | 全仓 `require_perm` / `has_permission` / `perm::require` 调用点去重 |
| 检查但未声明 | **0** | 63 个声明覆盖全部 61 个检查点（含迁移种子） |
| 声明但从不检查 | **2** | `system:dict:edit`：`crates/kernel/migrations/0004_dict.sql:59`，全仓无任何 `require_perm`/`has_permission` 引用；`system:notice:list`：`crates/kernel/migrations/0007_notice.sql:41`，仅在菜单种子 `crates/kernel/src/system.rs:166` 使用，`list_notices`（`crates/kernel/src/notices.rs:86-123`）不校验权限 |
| 校验实现 | 每模块各自实现，无路由层统一守卫 | `crates/kernel/src/system.rs:918-925`；`crates/kernel/src/posts.rs:395-402`；`crates/kernel/src/files.rs:331-337`；`crates/kernel/src/configs.rs:290-296`；`plugins/crm/src/perm.rs:7-13`。`system.rs:914-917` 自述"下一步升级为路由层 require_perm! 宏，避免忘记调用" |

#### 未受保护的写接口清单（9 个）

| # | 文件:行 | 方法路径 | 说明 |
|---|---|---|---|
| 1 | `crates/kernel/src/auth.rs:539` | `POST /api/auth/login` | 公开登录入口，按设计无需权限点 |
| 2 | `crates/kernel/src/auth.rs:540` | `POST /api/auth/logout` | 仅删除当前 Cookie 会话；无权限点、无审计 |
| 3 | `crates/kernel/src/auth.rs:542` | `POST /api/auth/tokens` | 创建个人访问令牌；仅操作 `user.tenant_id + user.user_id`（`auth.rs:1176-1188`），属自助接口 |
| 4 | `crates/kernel/src/auth.rs:543` | `DELETE /api/auth/tokens/{id}` | 吊销个人令牌；`WHERE id=$1 AND tenant_id=$2 AND user_id=$3`（`auth.rs:1218-1226`） |
| 5 | `crates/kernel/src/auth.rs:544` | `POST /api/auth/change-password` | 改自己的密码；`user: CurrentUser` + 原密码校验（`auth.rs:1264-1279`） |
| 6 | `crates/kernel/src/notices.rs:71` | `POST /api/system/notices/read-all` | 只写当前用户的已读记录（`notices.rs:202-231`） |
| 7 | `crates/kernel/src/notices.rs:72` | `POST /api/system/notices/{id}/read` | 先校验通知属于当前租户（`notices.rs:171-179`） |
| 8 | `crates/kernel/src/system.rs:384` | `PUT /api/system/audit-alert-subscriptions` | 只操作 `tenant_id + user_id` 的本人订阅（`system.rs:3618-3644`） |
| 9 | `crates/kernel/src/system.rs:388` | `PUT /api/system/audit-alert-subscription-config` | 只操作 `tenant_id + user_id` 的本人 Webhook（`system.rs:3788-3808`） |

> 上述 9 个接口没有"业务管理员才能做"的越权面，但违反 AGENTS 第 9 条"每个写接口必须有权限点"的字面要求；尤其 **创建/吊销 PAT 没有任何权限点或二次认证**，被盗会话可直接铸造长期令牌（见 P2-7）。

#### 写接口权限/审计矩阵（88 个）

> `权限=Y`：handler 内或注册的导入处理器中显式校验；`审计=Y`：handler 直接或通过 helper 调用 `audit::record*`；`system.rs:333` 的 `set_role_depts` 通过 `kernel_audit_role_depts`（`system.rs:2565,2570-2585`）审计，表中标 `Y*`。

| 文件 | 行号 | 路径 | 方法 | 权限 | 审计 |
|---|---:|---|---|---|---|
| crates/kernel/src/auth.rs | 539 | `/api/auth/login` | POST | N | Y |
| crates/kernel/src/auth.rs | 540 | `/api/auth/logout` | POST | N | N |
| crates/kernel/src/auth.rs | 542 | `/api/auth/tokens` | POST | N | N |
| crates/kernel/src/auth.rs | 543 | `/api/auth/tokens/{id}` | DELETE | N | N |
| crates/kernel/src/auth.rs | 544 | `/api/auth/change-password` | POST | N | Y |
| crates/kernel/src/configs.rs | 92 | `/api/system/configs` | POST | Y | N |
| crates/kernel/src/configs.rs | 94 | `/api/system/configs/{id}` | PUT | Y | Y |
| crates/kernel/src/exports.rs | 136 | `/api/system/export-tasks` | POST | Y | Y |
| crates/kernel/src/exports.rs | 141 | `/api/system/export-tasks/{id}` | DELETE | Y | N |
| crates/kernel/src/files.rs | 55 | `/api/system/files/upload` | POST | Y | Y |
| crates/kernel/src/files.rs | 56 | `/api/system/files/{id}` | DELETE | Y | N |
| crates/kernel/src/notices.rs | 69 | `/api/system/notices` | POST | Y | Y |
| crates/kernel/src/notices.rs | 71 | `/api/system/notices/read-all` | POST | N | N |
| crates/kernel/src/notices.rs | 72 | `/api/system/notices/{id}/read` | POST | N | N |
| crates/kernel/src/posts.rs | 81 | `/api/system/posts` | POST | Y | Y |
| crates/kernel/src/posts.rs | 82 | `/api/system/posts/{id}` | DELETE | Y | Y |
| crates/kernel/src/posts.rs | 82 | `/api/system/posts/{id}` | PUT | Y | Y |
| crates/kernel/src/posts.rs | 86 | `/api/system/users/{id}/posts` | PUT | Y | N |
| crates/kernel/src/system.rs | 249 | `/api/system/tenants` | POST | Y | Y |
| crates/kernel/src/system.rs | 250 | `/api/system/tenants/{id}/status` | PUT | Y | Y |
| crates/kernel/src/system.rs | 254 | `/api/system/tenants/{id}/quota` | PUT | Y | Y |
| crates/kernel/src/system.rs | 258 | `/api/system/tenants/purge` | POST | Y | Y |
| crates/kernel/src/system.rs | 259 | `/api/system/tenants/usage/capture` | POST | Y | N |
| crates/kernel/src/system.rs | 271 | `/api/system/tenants/bills/generate` | POST | Y | N |
| crates/kernel/src/system.rs | 280 | `/api/system/tenants/bills/backfill` | POST | Y | N |
| crates/kernel/src/system.rs | 284 | `/api/system/tenants/bills/overdue/scan` | POST | Y | N |
| crates/kernel/src/system.rs | 288 | `/api/system/tenants/bills/collections/reminders/scan` | POST | Y | N |
| crates/kernel/src/system.rs | 292 | `/api/system/tenants/bills/{id}/status` | PUT | Y | Y |
| crates/kernel/src/system.rs | 296 | `/api/system/tenants/bills/{id}/payments` | POST | Y | Y |
| crates/kernel/src/system.rs | 300 | `/api/system/tenants/bills/{id}/refunds` | POST | Y | Y |
| crates/kernel/src/system.rs | 304 | `/api/system/tenants/bills/{id}/collections` | POST | Y | Y |
| crates/kernel/src/system.rs | 308 | `/api/system/tenants/{id}/billing` | PUT | Y | Y |
| crates/kernel/src/system.rs | 315 | `/api/system/tenants/{id}` | DELETE | Y | Y |
| crates/kernel/src/system.rs | 316 | `/api/system/tenants/{id}/restore` | POST | Y | Y |
| crates/kernel/src/system.rs | 317 | `/api/system/users` | POST | Y | Y |
| crates/kernel/src/system.rs | 318 | `/api/system/users/{id}/status` | PATCH | Y | Y |
| crates/kernel/src/system.rs | 319 | `/api/system/users/{id}/roles` | PUT | Y | Y |
| crates/kernel/src/system.rs | 320 | `/api/system/users/{id}/unlock` | POST | Y | Y |
| crates/kernel/src/system.rs | 321 | `/api/system/roles` | POST | Y | N |
| crates/kernel/src/system.rs | 323 | `/api/system/sessions/{id}` | DELETE | Y | Y |
| crates/kernel/src/system.rs | 324 | `/api/system/roles/{id}/permissions` | PUT | Y | Y |
| crates/kernel/src/system.rs | 329 | `/api/system/password-policy` | PUT | Y | Y |
| crates/kernel/src/system.rs | 333 | `/api/system/roles/{id}/depts` | PUT | Y | Y* |
| crates/kernel/src/system.rs | 335 | `/api/system/menus` | POST | Y | Y |
| crates/kernel/src/system.rs | 337 | `/api/system/menus/{id}` | DELETE | Y | Y |
| crates/kernel/src/system.rs | 337 | `/api/system/menus/{id}` | PUT | Y | N |
| crates/kernel/src/system.rs | 338 | `/api/system/depts` | POST | Y | N |
| crates/kernel/src/system.rs | 339 | `/api/system/depts/{id}` | DELETE | Y | Y |
| crates/kernel/src/system.rs | 339 | `/api/system/depts/{id}` | PUT | Y | Y |
| crates/kernel/src/system.rs | 343 | `/api/system/audit-alert-config` | PUT | Y | Y |
| crates/kernel/src/system.rs | 347 | `/api/system/audit-alert-rules` | PUT | Y | Y |
| crates/kernel/src/system.rs | 351 | `/api/system/audit-alert-rules/preview` | POST | Y | N |
| crates/kernel/src/system.rs | 356 | `/api/system/audit-config/snapshots` | DELETE | Y | Y |
| crates/kernel/src/system.rs | 360 | `/api/system/audit-config/snapshots/{id}` | DELETE | Y | Y |
| crates/kernel/src/system.rs | 360 | `/api/system/audit-config/snapshots/{id}` | PUT | Y | N |
| crates/kernel/src/system.rs | 372 | `/api/system/audit-config/preview` | POST | Y | N |
| crates/kernel/src/system.rs | 376 | `/api/system/audit-config/import` | PUT | Y | Y |
| crates/kernel/src/system.rs | 380 | `/api/system/audit-webhook` | PUT | Y | Y |
| crates/kernel/src/system.rs | 384 | `/api/system/audit-alert-subscriptions` | PUT | N | N |
| crates/kernel/src/system.rs | 388 | `/api/system/audit-alert-subscription-config` | PUT | N | N |
| crates/kernel/src/system.rs | 396 | `/api/system/login-policy` | PUT | Y | Y |
| crates/kernel/src/system.rs | 402 | `/api/system/audit-deliveries/retry-dead` | POST | Y | Y |
| crates/kernel/src/system.rs | 406 | `/api/system/audit-deliveries/{id}/retry` | POST | Y | Y |
| crates/kernel/src/system.rs | 410 | `/api/system/audit-alerts/{id}/retry` | POST | Y | Y |
| crates/kernel/src/system.rs | 414 | `/api/system/audit-alerts/{id}/ack` | POST | Y | Y |
| crates/kernel/src/imports.rs | 418 | `/api/system/imports/{kind}` | POST | Y | Y |
| crates/kernel/src/system.rs | 419 | `/api/system/dicts` | POST | Y | N |
| crates/kernel/src/system.rs | 420 | `/api/system/dicts/{code}/items` | POST | Y | N |
| crates/kernel/src/user_import.rs | 86 | `/api/system/users/import` | POST | Y | Y |
| crates/kernel/src/user_import.rs | 88 | `/api/system/users/import-errors` | POST | Y | N |
| plugins/crm/src/contact.rs | 87 | `/api/crm/contacts` | POST | Y | Y |
| plugins/crm/src/contact.rs | 88 | `/api/crm/contacts/{id}` | DELETE | Y | Y |
| plugins/crm/src/contact.rs | 88 | `/api/crm/contacts/{id}` | PATCH | Y | Y |
| plugins/crm/src/contract.rs | 104 | `/api/crm/contracts` | POST | Y | Y |
| plugins/crm/src/contract.rs | 105 | `/api/crm/contracts/{id}` | DELETE | Y | Y |
| plugins/crm/src/contract.rs | 105 | `/api/crm/contracts/{id}` | PATCH | Y | Y |
| plugins/crm/src/api.rs | 229 | `/api/crm/customers` | POST | Y | Y |
| plugins/crm/src/api.rs | 230 | `/api/crm/customers/{id}` | DELETE | Y | Y |
| plugins/crm/src/api.rs | 230 | `/api/crm/customers/{id}` | PATCH | Y | Y |
| plugins/crm/src/excel.rs | 266 | `/api/crm/customers/import` | POST | Y | Y |
| plugins/crm/src/opportunity.rs | 90 | `/api/crm/opportunities` | POST | Y | Y |
| plugins/crm/src/opportunity.rs | 91 | `/api/crm/opportunities/{id}` | DELETE | Y | Y |
| plugins/crm/src/opportunity.rs | 91 | `/api/crm/opportunities/{id}` | PATCH | Y | Y |
| plugins/crm/src/payment.rs | 94 | `/api/crm/payments` | POST | Y | Y |
| plugins/crm/src/payment.rs | 95 | `/api/crm/payments/{id}` | DELETE | Y | Y |
| plugins/crm/src/payment.rs | 95 | `/api/crm/payments/{id}` | PATCH | Y | Y |
| plugins/demo/src/lib.rs | 131 | `/api/demo/items` | POST | Y | N |
| plugins/demo/src/lib.rs | 132 | `/api/demo/items/{id}` | DELETE | Y | N |
#### 权限点声明与检查交叉核对明细

- **内核权限点**不是通过 `permissions()` 声明，而是散落在迁移种子中：`0003_menu.sql:37-43`、`0004_dict.sql:57-59`、`0005_dept_scope.sql:78`、`0006_file.sql:34-36`、`0007_notice.sql:41-42`、`0008_audit_perm.sql:6`、`0009_config.sql:29-30`、`0017_menu_perm.sql:6-8`、`0018_export_task.sql:31`、`0019_dept_perm.sql:6-8`、`0020_post.sql:35-38`、`0024_session_permissions.sql:6-7`、`0027_audit_alert.sql:36`、`0054_tenant_permissions.sql:7-9`、`0057_tenant_delete_permission.sql:7`；常量镜像在 `crates/kernel/src/perms.rs:4-112`。
- **插件权限点**由 `Plugin::permissions()` 声明：CRM `plugins/crm/src/lib.rs:61-156`，demo `plugins/demo/src/lib.rs:107-122`，启动时 `sync_plugin_permissions` 落库（`crates/kernel/src/system.rs:2454-2475`）。
- **0 个"检查但未声明"**：所有 `require_perm`/`has_permission` 的字符串/常量都能在上述种子或插件声明中找到。`require_perm(&user, "system:config:list")` 这类字面量（`system.rs:5094,5128,5210,5382,5412`）同样有迁移声明。
- **2 个死权限点**：`system:dict:edit` 声明于 `0004_dict.sql:59`，但系统只有字典类型/字典项的新增和查询，没有更新/删除路由（`system.rs:419-423`）；`system:notice:list` 声明于 `0007_notice.sql:41`，仅用于菜单隐藏（`system.rs:166`），通知列表接口本身不校验（`notices.rs:86-123`）。

---

## 二、数据权限（行级/字段级）

### 结论

**CRM 业务查询的行级数据权限总体做得较完整（所有列表/更新/删除都带 `scope_filter`，6 张 CRM 表 RLS FORCE），但租户隔离并非"每个查询都带 tenant_id"：kernel 的身份/导出查询多处只靠 RLS，demo 插件整表没有 tenant_id/RLS，且 `sys_user_role`/`sys_role_permission`/`sys_user_post` 三张关联表没有 tenant_id；44 张业务表中 30 张 FORCE、3 张仅 ENABLE、11 张完全无 RLS。**

### 证据表

| 检查项 | 结论 | 证据 |
|---|---|---|
| `TenantTx` 使用 | 88 个写 handler 中 **76 个**在签名/首段接收 `TenantTx`（登录在 body 内自建 `TenantTx::begin_tenant`，`auth.rs:661`），实际 **77 个**走 TenantTx；不用的 11 个：登出、5 个平台维护任务、`sessions/{id}`、纯读取的 import-errors、demo 两个写接口 | `crates/kernel/src/tenant.rs:47-114`（`begin_tenant` 注入 `app.tenant_id`，`begin` 再注入 `app.user_id`，`set_config(..., true)` 事务级）；不使用清单见附录 A 的写接口表与 `system.rs:5932-6194`、`system.rs:6556-7651` |
| 直接 `&state.db` 绕过 RLS 的模块 | **demo 插件**全部业务 SQL 直连 `state.db`（无 `TenantTx`）；`system` 的租户计费域也直接 `state.db`，但属平台级跨租户查询 | demo：`plugins/demo/src/lib.rs:163-180`、`198-215`、`247-258`、`281-287`；system：`system.rs:6044-6051`、`6220`、`6343-6346`、`6420-6426`、`6606`；demo 表无 tenant_id：`plugins/demo/migrations/0001_init.sql:8-15` |
| 每个查询带 tenant_id？ | **否**。CRM 有 3 个辅助查询只靠 RLS；kernel 的 `list_users`/`update_user_status`/用户角色查询只靠 RLS 或完全没有租户条件 | CRM：`plugins/crm/src/api.rs:356-364`（`ensure_customer_visible` 无 `c.tenant_id`）、`api.rs:541-548`（合同回款汇总无 `c.tenant_id`）、`api.rs:857-864`（商机存在性无 `c.tenant_id`）；kernel：`system.rs:495-528`、`539-565`（用户列表/计数无 `u.tenant_id`）、`system.rs:782-791`（状态更新无 tenant 条件）、`system.rs:1148`、`1181-1199`（`sys_user_role` 按 user_id 直查/直改）、`auth.rs:912-923`、`944-954`（权限/数据范围只按 user_id） |
| 用户列表数据权限 | 有 `data_scope` 过滤，字段级仅 email/phone 脱敏 | `system.rs:495-528`、`539-565`、`574-585`；`USER_VIEW_SENSITIVE` 只在本处使用（`system.rs:575`） |
| CRM 行级数据权限 | 列表、详情、更新、删除均使用 `kernel::scope::condition`，`all/self/dept/custom/child` 五种分支齐全 | `plugins/crm/src/api.rs:14-16,45-60,122-131,244-262,312-319`；`contact.rs:115-145,158-186,225-249,341-372,417-442`；`contract.rs:129-169,182-207,242-266,346-394,432-465`；`opportunity.rs:118-158,171-196,231-255,346-378,423-448`；`payment.rs:119-161,178-203,238-264,342-390,430-457` |
| 字段级数据权限 | 只有 `sys_user.email/phone`；CRM 客户/联系人手机号、邮箱全部原样返回；用户导出绕过脱敏 | `system.rs:574-585`；`plugins/crm/src/api.rs:47-48`、`contact.rs:116`；`crates/kernel/src/exports.rs:217`（`system.users` 导出只要求 `system:user:list`）、`exports.rs:607-639`（导出 `u.email` 未脱敏） |

#### RLS 覆盖：44 张业务表

| 状态 | 数量 | 表清单 | 证据 |
|---|---:|---|---|
| **FORCE ROW LEVEL SECURITY** | **30** | 内核 24：`sys_config`、`sys_notice`、`sys_notice_read`、`sys_dept`、`sys_role`、`sys_dict_type`、`sys_dict_item`、`sys_audit_log`、`sys_file`、`sys_user`、`sys_menu`、`sys_role_dept`、`sys_audit_alert`、`sys_audit_alert_config`、`sys_audit_webhook`、`sys_audit_alert_rule`、`sys_audit_alert_subscription`、`sys_audit_alert_subscription_config`、`sys_audit_alert_delivery`、`sys_audit_config_snapshot`、`sys_api_token`、`sys_password_history`、`sys_password_policy`、`sys_login_policy`；CRM 6：`customer`、`contact`、`opportunity`、`contract`、`payment`、`opportunity_stage_history` | 内核：`0015_force_rls.sql:10-20`（11 张）+ `0021_audit_partition.sql:69-75` + `0022_role_dept.sql:18-24` + `0027_audit_alert.sql:27-33` + `0028_audit_alert_config.sql:17-23` + `0029_audit_webhook.sql:16-22` + `0030_audit_alert_rules.sql:36-42` + `0035_audit_alert_subscription.sql:14-20` + `0036_alert_delivery.sql:15-21,43-49` + `0039_audit_config_snapshot.sql:16-22` + `0043_api_token.sql:20-26` + `0046_password_history.sql:16-22` + `0047_password_policy.sql:15-21` + `0050_login_policy.sql:13-19`；CRM：`0003_rls.sql:8-16` + `0004_force_rls.sql:7-8` + `0005:32-39` + `0006:32-37` + `0007:31-36` + `0016:52-58` |
| **ENABLE 但未 FORCE** | **3** | `sys_export_task`、`sys_post`、`crm.payment_daily` | `crates/kernel/migrations/0018_export_task.sql:34-37`（注释明确"暂不 FORCE：recover_stale 需跨租户"）；`crates/kernel/migrations/0020_post.sql:41-46`（注释"先观察"）；`plugins/crm/migrations/0014_payment_daily.sql:31-35` |
| **完全无 RLS** | **11** | `sys_tenant`、`sys_session`、`sys_permission`、`sys_user_role`、`sys_role_permission`、`sys_user_post`、`sys_tenant_usage_snapshot`、`sys_tenant_bill`、`sys_tenant_bill_payment`、`sys_tenant_bill_collection`、`demo.demo_item` | 建表位置：`0001_init.sql:15-25`、`0025_sys_session.sql:8-20`、`0002_rbac.sql:39-46,53-68`、`0020_post.sql:26-30`、`0058_tenant_usage_snapshot.sql:5-16`、`0060_tenant_bill.sql:5-20`、`0061_tenant_bill_payment.sql:5-13`、`0065_tenant_bill_collection.sql:5-16`、`plugins/demo/migrations/0001_init.sql:8-15`；全仓无对应 `ALTER TABLE ... ROW LEVEL SECURITY` |
| **FORCE/ENABLE 差异的实际影响** | ENABLE-only 对表 owner（应用连接 `zcadmin`，`docker-compose.yml:15`）不生效；`0010_rls_config.sql:6-9` 自述"表 owner 仍可绕过策略" | `0010_rls_config.sql:6-9`；`docker-compose.yml:15-17,69-72` |
| `sys_audit_log` 分区 | 父表 FORCE；分区表未单独 FORCE，直接查分区会绕过策略 | `0021_audit_partition.sql:69-75`；分区 `sys_audit_log_*` 无策略 |

#### 关联表 tenant_id 清单

| 表 | tenant_id | RLS | 证据 |
|---|---|---|---|
| `sys_user_role` | **无** | 无 | `0002_rbac.sql:53-58`（仅 user_id/role_id/created_at，主键 `(user_id, role_id)`） |
| `sys_role_permission` | **无** | 无 | `0002_rbac.sql:63-68`（仅 role_id/permission_id/created_at） |
| `sys_user_post` | **无** | 无 | `0020_post.sql:26-30`（仅 user_id/post_id） |
| `sys_role_dept` | 有 | FORCE | `0022_role_dept.sql:8-13`（tenant_id + PK `(role_id, dept_id)`）、`0022:18-24` |
| `sys_user`、`sys_role`、`sys_dept`、`sys_file`、`sys_notice`、`sys_config`、`sys_menu`、`sys_session`、`sys_api_token` | 有 | 见 RLS 表 | `0001_init.sql:30-47`、`0002_rbac.sql:13-27`、`0005_dept_scope.sql`、`0006_file.sql`、`0007_notice.sql`、`0009_config.sql`、`0010_rls_config.sql`、`0025_sys_session.sql`、`0043_api_token.sql` |

> 重点：`sys_user_role` 无 tenant_id 且无 RLS，`set_user_roles` 又未调用 `ensure_user_in_tenant` 或校验 role 所属租户（`system.rs:1173-1200`），是本次审计中最直接的跨租户授权面（详见 P0-3）。

#### 租户计费/用量表（无 RLS，且 handler 直接 `state.db`）

| 表/接口 | 结论 | 证据 |
|---|---|---|
| `list_tenants` | 返回全部租户（`sys_tenant` 无 RLS），仅需 `system:tenant:list` | `system.rs:5542-5558` |
| `tenant_usage` | 路径 id 任意，直接查 `state.db`，仅需 `system:tenant:list` | `system.rs:6218-6222`；`tenant_usage.rs:139-159` |
| `tenant_bill` | 路径 id 任意，先查 `sys_tenant`，再按该租户 `set_config`，仅需 `system:tenant:list` | `system.rs:6333-6351` |
| `tenant_summary` | 路径 id 任意，直接 `state.db`，仅需 `system:tenant:list` | `system.rs:6043-6092` |
| `list_tenant_bills` | 查全部租户账单，无 tenant 条件 | `system.rs:6585-6606` |
| `update_bill_status` | 只按 `id` 查/改账单，无 tenant 条件 | `system.rs:6635-6658` |
| `create_bill_payment` / `create_bill_refund` | 只按 `bill_id` 查账单，无 tenant 条件 | `system.rs:6749-6805`、`6844-6900` |
| `capture_usage`/`generate_bills`/`backfill`/`scan` | 平台维护写接口无审计、部分无 TenantTx | `system.rs:6190-6195`、`6556-6566`、`7017-7047`、`7464-7471`、`7644-7651` |

---

## 三、会话与认证

### 结论

**会话存 DB（`sys.sys_session`，只存 token 的 SHA-256，30 秒进程内缓存），吊销不是即时一致：单会话 `revoke` 和 `logout` 都不清缓存，最长 30 秒仍可用；更严重的是禁用用户和修改角色/权限都不会吊销或刷新现有会话，按默认 8 小时 TTL 最长 8 小时不生效；Cookie 具备 HttpOnly + SameSite=Lax + release 下 Secure，但没有 CSRF Token/Origin 校验，登录只有账户级锁定、没有 IP/全局限流。**

### 证据表

| 检查项 | 结论 | 证据 |
|---|---|---|
| 会话存储 | **DB 表 `sys.sys_session`**，不是进程内 HashMap；表含 tenant_id/user_id/permissions/expires_at | `auth.rs:71-84`、`auth.rs:132-155`；`0025_sys_session.sql:8-20` |
| 令牌保护 | 只存 SHA-256 摘要，不存明文；Cookie 回传 | `auth.rs:105-114,142`、`auth.rs:1016-1027` |
| 缓存 | 进程内 30 秒 TTL；`get` 命中缓存跳过 DB | `auth.rs:75,161-193` |
| 吊销：批量用户/租户 | `revoke_user_sessions` / `revoke_tenant_sessions` 会 `invalidate_cache` | `auth.rs:214-242` |
| 吊销：单会话 | `revoke` 只 `DELETE` 不清缓存 → 本进程最长 30 秒仍可用；多实例各自缓存同样最长 30 秒 | `auth.rs:245-252`；`SessionStore::revoke` 无 `invalidate_cache` |
| 登出 | `remove` 也只 `DELETE` 不清缓存；且 `let _ =` 忽略 DB 错误 | `auth.rs:255-261,989-991` |
| 过期清理 | 后台每 10 分钟 `purge_expired`，有删除才清缓存 | `jobs.rs:27-42`、`auth.rs:264-272` |
| 禁用用户 | `update_user_status` 只改 `sys_user.status`，**不调用 `revoke_user_sessions`**；`CurrentUser` 提取只读会话，不复查用户状态 | `system.rs:757-816`；`auth.rs:350-381` |
| 修改角色/权限 | `set_user_roles`/`set_role_permissions` 不吊销会话，也不刷新 `sys_session.permissions`；权限在登录时快照 | `system.rs:1057-1103,1166-1214`；`auth.rs:832-867` |
| 会话 TTL | 默认 8 小时（28800 秒） | `config.rs:215`；`config/default.toml:38` |
| 停用租户 | 会 `revoke_tenant_sessions`，这是做得对的部分 | `system.rs:5712-5714` |
| 改密 | 改密后吊销其他会话（`revoke_tokens` 时同时吊销 PAT） | `auth.rs:1331-1351` |
| Cookie | `HttpOnly; SameSite=Lax; Path=/`；`Secure` 只在 `!cfg!(debug_assertions)` 时追加；无 `__Host-` 前缀 | `auth.rs:1011-1027` |
| 登录锁定 | 按租户 `sys_login_policy`：默认 5 次失败锁 900 秒；锁定检查在密码校验前 | `auth.rs:604-635,688-752`；`0050_login_policy.sql` |
| 密码策略 | Argon2id（`Argon2::default()`，OWASP m=19456/t=2/p=1）；默认最小 10 位、大小写+数字、历史 5 次、有效期默认 0（不过期） | `password.rs:30-52,81-121,125-156` |
| 批量导入密码策略 | **绕过租户策略**：`user_import.rs:426` 使用 `validate_strength`（默认策略），不是 `load_policy` | `user_import.rs:426` vs `system.rs:658-661` |
| CSRF | 只有 `SameSite=Lax`；`X-CSRF-Token` 仅在 CORS 允许头里出现（`app.rs:283`），全仓无读取/校验；无 Origin/Referer 校验 | `auth.rs:1023`；`app.rs:279-285`；`grep` 无 `Origin`/`csrf` 校验代码 |
| 限流 | **无任何 rate limit 中间件**；`AppError::RateLimited` 只定义未使用；登录仅账户级锁定，可被 5 次错误密码锁死任意已知账号 | `app.rs:114-132`；`error.rs:49-51,74,88` 无构造点 |
| 用户名枚举 | 登录失败统一 401，但用户不存在时在密码校验前直接返回（`auth.rs:678-681`），存在时执行 Argon2（`auth.rs:698`），响应时间可区分 | `auth.rs:663-681,696-698` |
| IP 采集 | 信任客户端 `X-Forwarded-For`/`X-Real-IP`，无可信代理白名单；审计 IP 可伪造 | `auth.rs:1050-1062`；`audit.rs:129`（`$7::text::inet`，非法 IP 会让审计写入失败） |

---

## 四、审计

### 结论

**审计不是与业务同事务：`audit::record*` 自己在连接池上开独立事务，且固定写 `result='success'`；88 个写路由中至少 61 个有审计、27 个没有，其中 18 个 handler 在 `tx.commit()` 之前就写审计（业务回滚仍会留下"成功"日志），因此审计只能作为"有记录"的参考，不能作为强一致合规证据。**

### 证据表

| 检查项 | 结论 | 证据 |
|---|---|---|
| 审计写入方式 | 独立事务，`pool.begin()` + `set_config` + `INSERT`，失败只 `tracing::error!`，不影响业务 | `audit.rs:14-16,106-152`（尤其 `118-146`、`148-151`） |
| 结果字段 | 固定 `'success'`；没有失败/回滚审计 | `audit.rs:124-130`（`VALUES (..., 'success', ...)`） |
| 与业务同事务 | **0 个**；全部通过 `&state.db` 新事务 | `audit.rs:118-146`；所有调用点传 `&state.db`（如 `system.rs:715-723`、`files.rs:356-364`） |
| 审计覆盖率 | **61/88**（69.3%）有审计；27 个无审计（其中 3 个是 preview/错误导出，非业务变更） | 全量矩阵见上一节；无审计清单见附录 B |
| 先审计后提交的写接口 | **18 个**：`configs.rs` 更新配置、`notices.rs` 发布通知、`system.rs` 设置角色部门，以及 CRM 15 个 create/update/delete | `configs.rs:221-263`；`notices.rs:233-276`；`system.rs:2565-2566`；`plugins/crm/src/api.rs:201-210,283-293,336-346`；`contact.rs:295-305,382-392,453-463`；`contract.rs:314-324,404-414,476-486`；`opportunity.rs:301-311,387-397,458-468`；`payment.rs:310-321,396-406,467-477` |
| 回滚仍写成功日志 | **会**：上述 18 个点 audit 在 commit 前执行；若 commit 失败，审计已独立提交且 `result='success'` | 同上；`audit.rs:143` 提交后返回 |
| 登录失败审计 | 失败登录在业务事务提交后记录 `auth.login.failed` / `auth.login.locked`，但 `result` 仍是 success、action 表达失败 | `auth.rs:752-777` |
| 审计 IP/UA | `record_user` 使用**登录时**写入会话的 IP/UA，不是当前请求 IP/UA；同一会话后续操作的审计 IP 恒为登录 IP | `auth.rs:82-102`；`Session` 的 ip/ua 在登录时赋值 `auth.rs:854-867` |
| 审计可被伪造/打断 | `X-Forwarded-For` 未校验，非法值导致 `$7::text::inet` 转换失败，`record_with_meta` 只打日志 → 失败登录审计可被攻击者抑制 | `auth.rs:1050-1062`、`audit.rs:129,148-151` |
| 记录内容 | CRM update 有整行 before/after 快照；文件上传只记 id/size/mime；登录记 IP/UA | `plugins/crm/src/api.rs:237,281-291`；`files.rs:354-365`；`auth.rs:890-901` |

#### 无审计的高风险写接口（节选）

| 文件:行 | 路径 | 风险 |
|---|---|---|
| `crates/kernel/src/system.rs:321` | `POST /api/system/roles` | 新建角色（授权基础）无审计 |
| `crates/kernel/src/system.rs:337` | `PUT /api/system/menus/{id}` | 菜单/权限入口变更无审计 |
| `crates/kernel/src/system.rs:338` | `POST /api/system/depts` | 组织架构变更无审计 |
| `crates/kernel/src/system.rs:419-420` | `POST /api/system/dicts`、`POST /api/system/dicts/{code}/items` | 字典数据变更无审计 |
| `crates/kernel/src/configs.rs:92` | `POST /api/system/configs` | 参数新增无审计（更新有） |
| `crates/kernel/src/files.rs:56` | `DELETE /api/system/files/{id}` | 文件删除无审计（上传有） |
| `crates/kernel/src/posts.rs:86` | `PUT /api/system/users/{id}/posts` | 用户岗位变更无审计 |
| `crates/kernel/src/auth.rs:542-543` | `POST/DELETE /api/auth/tokens` | 长期令牌创建/吊销无审计 |
| `plugins/demo/src/lib.rs:131-132` | `POST/DELETE /api/demo/items` | demo 全部写操作无审计 |

> 注：`set_role_depts` 实际通过 `kernel_audit_role_depts`（`system.rs:2565,2570-2585`）记录，不属于无审计；矩阵脚本曾把它误判为无审计，最终统计已扣除。

---

## 五、错误处理与安全基线

### 结论

**统一错误模型对 5xx 隐藏 sqlx/anyhow 细节（做得对），但导入/导出任务把底层数据库错误文本回传给客户端；应用层没有安全响应头、没有限流、`/metrics`、`/docs`、`/api/openapi.json` 无鉴权；文件下载有租户/所有者校验且存储层拒绝 `..`，未发现可利用的 SQL 注入或 ORDER BY 注入。**

### 证据表

| 检查项 | 结论 | 证据 |
|---|---|---|
| 5xx 错误泄漏 | `AppError::Database/Internal` 对外固定文案，详情只进日志 | `error.rs:93-106,125-145` |
| 4xx/导入错误泄漏 | `imports.rs` 把插件 `anyhow` 错误转 `AppError::Validation(e.to_string())`；`user_import` 把 `写入失败: {e}` 放进返回的 errors 数组；导出任务把 `e.to_string()` 存进 `error` 字段供前端读取 | `imports.rs:109-111`；`user_import.rs:495-501`；`exports.rs:290-293,486-497`；`ExportTaskVo.error` `exports.rs:103` |
| 安全响应头 | **无** CSP / HSTS / X-Frame-Options / X-Content-Type-Options / Referrer-Policy / Permissions-Policy；`app.rs` 中间件只有 Timeout/Trace/RequestId/Metrics/CORS | `app.rs:114-132`；全仓 `grep` 0 命中 |
| 限流 | **无**（`AppError::RateLimited` 只定义未构造） | `app.rs:114-132`；`error.rs:49-51,74,88` |
| `/metrics` 鉴权 | **无**，任何人可拉取请求量/延迟/DB 连接池指标 | `app.rs:95-98`；`metrics.rs:54-63`（仅 `State`，无 `CurrentUser`） |
| `/docs` / `/api/openapi.json` | **无鉴权**，公开 API 契约 | `app.rs:99-103`；`openapi.rs:serve/docs` |
| 文件下载权限 | 文件：`tenant_id + (uploader_id = 当前用户 OR FILE_LIST)`；导出任务：`tenant_id + user_id`；越权返回 404 | `files.rs:269-280`；`exports.rs:313-321` |
| 文件级 ACL | 没有文件级/分享级 ACL：有 `FILE_LIST` 即可读同租户所有文件 | `files.rs:246,277`（`uploader_id = $3 OR $4::bool`） |
| 路径穿越 | `resolve` 拒绝空 key、绝对路径、`..`；key 由服务端 `{tenant_id}/{uuid}` 生成，有单测 | `storage.rs:36-42,85-93`；`files.rs:177-185` |
| 上传 MIME | `mime` 来自客户端并原样存库/回显；下载有 `Content-Disposition: attachment`，但无 `nosniff` | `files.rs:136-139,285-290` |
| SQL 注入/ORDER BY | 未发现用户可控 SQL 拼接或 ORDER BY；`scope::condition` 只替换 `$n` 占位符，`sort` 是 i32 绑定参数 | `scope.rs:16-44`；`db.rs:36-44`（schema 白名单）；`tenant_purge.rs:97-121`（catalog 名加引号）；全仓无 `ORDER BY {` |
| CORS | 显式 origin 白名单 + `allow_credentials(true)`，未用 `*`；允许 `x-csrf-token` 但未校验 | `app.rs:258-285` |
| 默认凭据 | `config/default.toml:35` `dev_password = "admin123456"`；`docker-compose.yml:15-17` DB `zcadmin/zcadmin`、`docker-compose.yml:72` `AUTH_DEV_PASSWORD` 默认 `admin123456` | `config/default.toml:35`；`docker-compose.yml:15-17,69-72` |
| CSRF | 仅 SameSite=Lax；无 token/Origin 校验 | `auth.rs:1023`；`app.rs:283` |

---

## 六、分层与规模

### 结论

**AGENTS 第 8 条"所有查询必须经过 Repository、禁止在 handler 写 SQL"在仓库中系统性未执行：全仓没有 `repo/`/`service/` 目录，没有 `Repository`/`Service` 结构体；`system.rs` 一个文件 7655 行、198 处 `sqlx::query`、36 个超过 50 行的函数，handler 直接写 SQL 是常态。**

### 证据表

| 排名 | 文件 | 行数 | `sqlx::query` 数 | >50 行函数数/总函数数 | 最长函数（起始行，行数） | 证据 |
|---:|---|---:|---:|---:|---|---|
| 1 | `crates/kernel/src/system.rs` | **7655** | **198** | **36 / 114** | `export_tenant_bills_xlsx`（`system.rs:7114`，270 行） | 文件头/路由 `system.rs:247-424`；SQL 例 `system.rs:495-537`、`2082-2113`、`5544-5558`、`6590-6606` |
| 2 | `crates/kernel/src/auth.rs` | **1363** | 35 | 4 / 36 | `login`（`auth.rs:649`，257 行） | `auth.rs:663-871` 直接写 7 段 SQL；`auth.rs:1176-1196` 令牌 SQL |
| 3 | `plugins/crm/src/api.rs` | **1214** | 29 | 5 / 19 | `dashboard_summary`（`api.rs:645`，163 行） | `api.rs:45-98`、`122-142`、`244-293` 等 |
| 4 | `crates/kernel/src/audit.rs` | **766** | 34 | 3 / 18 | `dispatch_subscriptions`（`audit.rs:550`，179 行） | `audit.rs:118-152`、`188-250` 等 |
| 5 | `crates/kernel/src/exports.rs` | **652** | 11 | 3 / 11 | `create_task`（`exports.rs:208`，90 行） | `exports.rs:168-198`、`235-251`、`429-481` |

| 分层检查项 | 结论 | 证据 |
|---|---|---|
| Repository/Service 层 | **不存在**；全仓无 `repo/`、`repository/`、`service/`、`domain/` 目录，无 `struct *Repository`/`struct *Service`/`trait *Repository` | 目录扫描 `crates/kernel/src`、`plugins/crm/src`、`plugins/demo/src`；`grep` 0 命中 |
| handler 内 SQL | 是；例如用户列表 3 段 SQL、审计列表 2 段 SQL、CRM 列表 `format!` 拼 `scope_filter` | `system.rs:495-572`、`system.rs:2082-2113`、`plugins/crm/src/api.rs:45-98`、`files.rs:81-101` |
| 文件 ≤300 行 | **17 个文件超限**：`system.rs` 7655、`auth.rs` 1363、`crm/api.rs` 1214、`audit.rs` 766、`exports.rs` 652、`crm/import.rs` 549、`user_import.rs` 511、`contract.rs` 487、`payment.rs` 479、`opportunity.rs` 470、`contact.rs` 464、`posts.rs` 449、`excel.rs` 385、`files.rs` 374、`demo/lib.rs` 330、`crm/lib.rs` 322、`configs.rs` 305 | 逐文件行数统计（`read` 工具 total） |
| 函数 ≤50 行 | `system.rs` 36 个超 50 行（最长 270）；`auth.rs` 4 个；`crm/api.rs` 5 个 | 函数体括号配对统计；顶部见上表 |
| `unsafe`/`missing_docs` | 内核/插件入口均 `#![forbid(unsafe_code)]` + `#![deny(missing_docs)]`（做得对） | `crates/kernel/src/lib.rs:31-33`、`plugins/crm/src/lib.rs:17-18`、`plugins/demo/src/lib.rs:25-26` |
| 内核依赖插件 | `crates/kernel/Cargo.toml` 无任何 `plugin-*` 依赖（符合铁律 1） | `crates/kernel/Cargo.toml` 依赖列表 |
| `unwrap/expect` | 非测试仍有 `TenantTx` 的 `expect` 与 `tenant_purge` 的 `unwrap` 等；不影响主流程但违反 AGENTS 第 4 条 | `tenant.rs:92,98`；`system.rs:7042,7044`；`tenant_purge.rs:149-155` |

---

## 七、P0–P2 问题清单

### P0（生产前必须修复）

**P0-1 禁用用户不吊销会话，最长 8 小时仍可操作**
- 证据：`system.rs:757-816`（`update_user_status` 只改状态）；`auth.rs:350-381`（`CurrentUser` 只从会话取权限，不查 `sys_user.status`）；`auth.rs:832-867`（登录时快照权限）；`config.rs:215` / `config/default.toml:38`（TTL 28800 秒）。
- 影响：管理员"禁用"一个已登录用户后，对方仍可继续读写、导出、调用管理接口最长 8 小时；PAT 路径会查状态（`auth.rs:441-443`）但 Cookie 路径不会。
- 修复：`update_user_status` 在状态为 `disabled/locked` 时调用 `sessions.revoke_user_sessions`；或在 `CurrentUser::from_request_parts` 每次校验用户状态/权限版本；引入 `session_version`。

**P0-2 平台级权限与租户角色同域，租户管理员可自我提权到平台级**
- 证据：`system.rs:894-904`（`list_permissions` 返回全部权限点，含 `system:tenant:*`）；`system.rs:1057-1088`（`set_role_permissions` 接受任意 `permission_id`，只校验角色属于本租户 `ensure_role_in_tenant` 1107-1125）；`system.rs:1166-1214`（可把任意角色绑给任意用户，见 P0-3）；平台接口仅用 `require_perm(TENANT_*)`（`system.rs:5542,5692,5817,6194` 等），没有独立的平台管理员判定。
- 影响：任何拥有 `ROLE_EDIT`（通常还有 `USER_ADD/USER_EDIT`）的租户管理员，可给本租户角色授予 `system:tenant:list/edit/delete`，再新建用户/绑定该角色并登录，进而读取/修改/删除所有租户（`list_tenants` 返回全量，`system.rs:5544-5558`；`tenant_summary` 可传任意 tenant id，`6043-6092`；`list_tenant_bills` 无 tenant 条件，`6590-6606`）。
- 修复：把平台权限（`system:tenant:*`、租户账单/用量）与租户权限分域；`sys_permission` 增加 `scope=platform|tenant`；`list_permissions` 按 scope 过滤；平台接口额外校验 `is_super` 或平台管理员标记。

**P0-3 `sys_user_role` 无 tenant_id/RLS，`set_user_roles` 未做租户校验 → 跨租户角色篡改**
- 证据：`0002_rbac.sql:53-58`（无 tenant_id、无 RLS）；`system.rs:1145-1151`（按 user_id 直查角色）；`system.rs:1173-1200`（未调用 `ensure_user_in_tenant`，也未校验 `role_id` 属于当前租户；只拒绝改自己 `1176-1178`）；对比 `posts.rs:335-336,362-363` 的正确做法（有 `ensure_user_in_tenant`）。
- 影响：租户 A 的 `USER_EDIT` 持有者可通过已知/泄漏的 user_id 修改租户 B 用户的角色；也可把本租户用户绑定到租户 B 的角色（`sys_role_permission` 无 tenant_id，权限按 role_id 全局连接），造成跨租户权限混合。
- 修复：给 `sys_user_role` 增加 `tenant_id` + RLS FORCE；`set_user_roles` 必须先 `ensure_user_in_tenant`，并逐个 `ensure_role_in_tenant`；`get_user_roles` 同样校验。

**P0-4 demo 插件整表无 tenant_id/RLS，所有租户共享数据**
- 证据：`plugins/demo/migrations/0001_init.sql:8-15`（`demo_item` 只有 id/name/note/created_at/created_by/deleted_at，无 tenant_id/version/updated_*）；`plugins/demo/src/lib.rs:163-180,206-210,247-258,281-287`（全部 `&state.db`，无 TenantTx/tenant 条件）；`apps/server/src/main.rs:22-23` 强制链接 demo 插件，默认随服务启动。
- 影响：多租户部署下，任何租户登录用户都能读取/修改/删除所有租户的 demo 数据；demo 是 AGENTS 指定的"唯一权威样例"，会复制出更多同类表。
- 修复：`demo_item` 增加 tenant_id + RLS FORCE，handler 改用 `TenantTx`；或在生产构建中不链接 demo。

### P1（高风险/系统性）

**P1-1 审计与业务不同事务，18 个 handler 回滚仍写成功日志**
- 证据：`audit.rs:118-146`（独立 pool 事务）、`audit.rs:129`（固定 success）；先审计后 commit 的 18 个点见第四节。
- 影响：合规审计不可信；业务失败/回滚会留下成功记录，攻击者可利用"审计显示成功但数据未变"混淆溯源。
- 修复：audit 写入改为与业务同一 `TenantTx`（outbox 或同事务 INSERT）；失败时写 `result='failed'` 并在业务失败路径调用；至少把 audit 移到 `tx.commit()` 之后。

**P1-2 审计可通过 `X-Forwarded-For` 伪造/抑制**
- 证据：`auth.rs:1050-1062` 直接信任 XFF；`audit.rs:129` `$7::text::inet`；非法 IP 使 `record_with_meta` 的 INSERT 失败，`audit.rs:148-151` 只打日志。
- 影响：暴力破解失败登录可被伪造 XFF 使审计写入失败；成功登录的审计 IP 可任意伪造。
- 修复：只信任可信代理并校验 IP 格式；优先使用 socket peer addr；审计 IP 列改为 text 或先校验后写入。

**P1-3 RLS 覆盖不完整：11 张表无 RLS、3 张仅 ENABLE**
- 证据：见第二节 RLS 表；尤其 `sys_user_role`/`sys_role_permission`/`sys_user_post`、`sys_tenant_usage_snapshot`、`sys_tenant_bill*` 无 RLS；`sys_export_task`、`sys_post`、`crm.payment_daily` 仅 ENABLE（owner 可绕过，`0010_rls_config.sql:6-9`）。
- 影响：只要应用连接是表 owner 或某条查询绕开 TenantTx，这些表就无数据库兜底；`list_users`/`update_user_status` 正依赖 `sys_user` 的 FORCE 才没有跨租户泄漏。
- 修复：所有含 tenant_id 的业务表统一 ENABLE + FORCE；`sys_user_role`/`sys_role_permission`/`sys_user_post` 增加 tenant_id 并 FORCE；移除或补齐 `sys_export_task`/`sys_post`/`payment_daily` 的 FORCE 例外（用平台专用连接处理跨租户任务）。

**P1-4 CRM 写接口对父实体只校验 tenant_id，不校验行级可见性（写侧 IDOR）**
- 证据：`contact.rs:275-293`（直接绑定 `req.customer_id`）、`opportunity.rs:281-299`、`contract.rs:292-312`、`payment.rs:290-308`；只有 3 个读辅助调用 `ensure_customer_visible`（`api.rs:351-364,394,426,824`），create/update 未调用。
- 影响：有 `contact:add/opportunity:add/contract:add/payment:add` 的用户可把数据挂到本租户内自己无权查看的客户/合同（甚至通过 UUID 指向其他租户，RLS 只校验 tenant_id 而父实体可能属于别租户的 tenant_id 记录）；更新时也可改父 id。
- 修复：create/update 对 `customer_id`/`contract_id` 调用统一的 `ensure_*_visible`（tenant + data_scope）；父实体不存在或无权限统一 404。

**P1-5 权限变更/单会话吊销不是即时生效**
- 证据：`set_role_permissions`/`set_user_roles` 无 session 刷新（`system.rs:1057-1103,1166-1214`）；`revoke`/`remove` 无 `invalidate_cache`（`auth.rs:245-261`）；缓存 30 秒（`auth.rs:75`）；多实例各自缓存。
- 影响：权限收回/角色变更最长 8 小时不生效；强制下线/登出后 30 秒内 token 仍可用；多实例部署更久。
- 修复：角色/权限/用户状态变更时 `revoke_user_sessions`；`revoke`/`remove` 清缓存；多实例用 Redis pub/sub 或 DB 版本号，缓存 TTL 降到秒级。

**P1-6 登录无 IP/全局限流，账户锁定可被用作 DoS，且存在时序用户名枚举**
- 证据：`app.rs:114-132` 无 rate limit；`auth.rs:688-752` 按账号 `failed_attempts` 锁定；`auth.rs:678-681` 用户不存在直接返回 vs `auth.rs:698` 执行 Argon2。
- 影响：攻击者可对任意已知用户名发 5 次错误密码将其锁定 15 分钟；可无限尝试不同用户名；时序可枚举账号。
- 修复：加入 IP/租户维度限流与指数退避；不存在的用户也执行一次 dummy Argon2 校验；锁定策略加验证码/告警。

**P1-7 `system:user:list` 可导出未脱敏邮箱，绕过 `USER_VIEW_SENSITIVE`**
- 证据：`system.rs:574-585` 只在列表脱敏；`exports.rs:217` 的 `system.users` 导出只要求 `system:user:list`；`exports.rs:607-639` 直接 `SELECT u.email` 并写入 xlsx，且 `export_users` 没有 `data_scope` 过滤（`exports.rs:606-615`）。
- 影响：无敏感字段权限、甚至只有 `self` 数据范围的用户，可通过异步导出拿到全租户邮箱与用户列表。
- 修复：导出复用同一字段级策略；要求 `USER_VIEW_SENSITIVE` 或对导出字段脱敏。

**P1-8 平台租户接口向 `system:tenant:list` 持有者暴露全量租户数据**
- 证据：`list_tenants` 无 tenant 过滤（`system.rs:5544-5558`）；`tenant_summary` 可传任意 id（`6043-6092`）；`list_tenant_bills` 无 tenant 过滤（`6590-6606`）；`tenant_bill`/`tenant_usage` 任意 id（`6333-6351,6218-6222`）。
- 影响：与 P0-2 组合即跨租户数据泄漏；即使不自我提权，误授 `system:tenant:list` 的租户运营也会看到全部租户的配额/账单/用量。
- 修复：平台级接口单独权限/角色；租户自助接口按 `user.tenant_id` 限制 id。

**P1-9 主数据/组织关联可跨租户引用**
- 证据：`create_dept` 接收任意 `parent_id` 未校验（`system.rs:1807-1830`）；`create_user` 接收任意 `dept_id` 未校验（`system.rs:679-700`）；`set_role_depts` 接收任意 `dept_ids` 未校验（`system.rs:2554-2562`）；`set_user_roles` 见 P0-3。
- 影响：可构造跨租户的部门树/用户归属/角色数据范围引用，配合 child/custom scope 可能扩大可见范围或造成数据污染。
- 修复：所有外键 id 在写入前按 `tenant_id` 校验；数据库层增加复合外键 `(tenant_id, id)`。

**P1-10 单会话强制下线/登出 30 秒延迟（与 P1-5 部分重叠，单列以便修复验收）**
- 证据：`auth.rs:245-252` `revoke` 不清缓存；`auth.rs:255-261` `remove` 不清缓存；`auth.rs:161-171` 命中 30 秒缓存；`auth.rs:989-991` 忽略 DB 错误。
- 影响：管理员"强制下线"或用户"退出登录"后 token 仍可用最长 30 秒；多实例同样。
- 修复：`revoke`/`remove` 调用 `invalidate_cache`；登出失败要返回错误或重试；多实例共享吊销版本。

### P2（加固/一致性/文档）

| # | 问题 | 证据 |
|---|---|---|
| P2-1 | 无安全响应头（CSP/HSTS/X-Frame-Options/X-Content-Type-Options/Referrer-Policy/Permissions-Policy） | `app.rs:114-132`；全仓 0 命中 |
| P2-2 | `/metrics`、`/docs`、`/api/openapi.json` 无鉴权；`/metrics` 暴露请求量/延迟/DB 池 | `app.rs:95-103`；`metrics.rs:54-63` |
| P2-3 | 无 CSRF Token/Origin 校验；仅 SameSite=Lax；`X-CSRF-Token` 在 CORS 允许头中但从不读取 | `auth.rs:1023`；`app.rs:279-285` |
| P2-4 | 错误文本泄漏：导入逐行错误、导出任务 `error` 字段包含 sqlx 错误 | `imports.rs:109-111`；`user_import.rs:495-501`；`exports.rs:290-293,486-497` |
| P2-5 | 27 个写路由无审计，含 `create_role`/`create_dept`/`update_menu`/`create_dict_*`/`delete_file`/`set_user_posts`/PAT 创建吊销 | 附录 B |
| P2-6 | 死权限点 `system:dict:edit`、`system:notice:list`；后者仅用于菜单隐藏，通知 API 实际对所有登录用户开放 | `0004_dict.sql:59`；`0007_notice.sql:41`；`system.rs:166`；`notices.rs:86-123` |
| P2-7 | PAT 无二次认证、可创建无过期时间令牌；被盗会话可铸造长期凭据 | `auth.rs:1144-1201`（`expires_in_days=None` 允许不过期） |
| P2-8 | 字段级权限只覆盖 `sys_user` 列表；CRM 手机号/邮箱原样返回；导出绕过脱敏（P1-7） | `system.rs:574-585`；`plugins/crm/src/api.rs:47-48`；`contact.rs:116` |
| P2-9 | `update_user_status` 的 UPDATE、`list_users` 的列表/计数无显式 tenant 条件，只依赖 `sys_user` FORCE；防御纵深不足 | `system.rs:782-791,495-528,539-565` |
| P2-10 | `ops/disable_rls.sql` 只覆盖 11 张内核 + 2 张 CRM 表，未覆盖 0015 之后新增的 17 张 FORCE 表；紧急回滚不完整 | `ops/disable_rls.sql:10-23`；对比 `0015_force_rls.sql:10-20` 与 0021-0050 各迁移 |
| P2-11 | `tenant.rs:31-35` 自述"目前只有 configs 使用 TenantTx"、`auth.rs:9` 自述"会话存进程内 HashMap"均与实现不符；误导后续开发 | `tenant.rs:31-35`；`auth.rs:7-11` |
| P2-12 | AGENTS 提到的 `cargo xtask check-perms` 未实现；`arch-check` 只检查 R1/R2，不检查 300 行/unsafe/生成物哈希 | `crates/xtask/src/main.rs:26-40,59-69,191-246` |
| P2-13 | `create_role` 的 `data_scope` 无枚举校验，可传任意字符串；`create_user`/`create_dept`/`set_role_depts` 外键无租户校验 | `system.rs:942-949,992`；`system.rs:697`；`system.rs:1827`；`system.rs:2554-2562` |
| P2-14 | 批量导入使用默认密码策略而非租户策略；`user_import.rs:426` | `user_import.rs:426` vs `system.rs:658-661` |
| P2-15 | 默认凭据/数据库口令进入 compose：`zcadmin/zcadmin`、`AUTH_DEV_PASSWORD` 默认 `admin123456`；release 下 `admin123456` 因无大写会被拒绝启动，但开发/演示环境仍弱口令 | `docker-compose.yml:15-17,69-72`；`config/default.toml:35`；`system.rs:66-73` |
| P2-16 | `TenantTx`/`tenant_purge` 非测试 `expect/unwrap`；`TenantTx` 设计上不可达，但违反 AGENTS 第 4 条 | `tenant.rs:92,98`；`tenant_purge.rs:149-155`；`system.rs:7042,7044` |
| P2-17 | 登录用户不存在时提前返回，存在时执行 Argon2，存在时序差异 | `auth.rs:678-681,698` |
| P2-18 | `sys_session` 按设计无 RLS（0025 注释），但多实例/缓存使会话吊销最终一致而非强一致 | `0025_sys_session.sql:3-5`；`auth.rs:75,161-193` |

---

## 八、已做对的部分（防止只看到问题）

1. **密码哈希**：Argon2id + 随机盐，OWASP 参数，PHC 格式，`verify_password` 对非法哈希返回 false 不 panic（`password.rs:30-52`）。
2. **会话令牌保护**：DB 只存 SHA-256，不存明文；Cookie `HttpOnly + SameSite=Lax`，release 加 `Secure`（`auth.rs:105-114,1016-1027`）。
3. **登录防枚举/锁定**：统一 401；锁定检查在密码校验前；`sys_login_policy` 支持租户级 5 次/900 秒（`auth.rs:677-694,604-635`）。
4. **RLS 主体覆盖**：30 张表 FORCE，含 `sys_user`、`sys_role`、`sys_dept`、`sys_file`、`sys_audit_log` 与 CRM 6 张核心表（`0015_force_rls.sql:10-20`、CRM `0004_force_rls.sql:7-8` 等）。
5. **TenantTx 统一上下文**：`set_config(..., true)` 事务级，提交/回滚自动清除，避免连接池串租户（`tenant.rs:54-62`）。
6. **写接口权限覆盖率**：79/88 有显式权限；CRM 16 个写接口 100% `perm::require`（`plugins/crm/src/api.rs:38,121,165,236,311` 等）。
7. **权限声明与检查一致性**：63 个声明、61 个检查、0 个检查未声明；`sync_plugin_permissions` 幂等 upsert（`system.rs:2454-2475`），无"代码检查但库中无权限点"的漂移。
8. **统一错误模型**：`AppError` 5xx 不泄露 SQL/堆栈，RFC 9457 `application/problem+json`（`error.rs:93-145`）。
9. **CORS**：显式 origin 白名单 + `allow_credentials(true)`，未使用 `*`（`app.rs:258-285`）。
10. **文件存储安全**：key 由服务端 `{tenant}/{uuid}` 生成，`resolve` 拒绝绝对路径/`..`，有路径穿越单测（`storage.rs:36-42,85-93`；`files.rs:177-185`）。
11. **审计基础能力**：审计表按月分区、保留 180 天、独立租户事务写入、CRM update 有 before/after 快照（`audit.rs:118-146`、`app.rs:148-166`、`plugins/crm/src/api.rs:237,281-291`）。
12. **租户停用**：`update_tenant_status` 在 `suspended` 时吊销该租户全部会话（`system.rs:5712-5714`）。
13. **软删与版本号**：CRM 更新用 `version = version + 1`，删除为软删；文件删除物理失败不回滚元数据（`files.rs:321-326`；CRM 各 update 语句）。
14. **内核依赖方向**：`crates/kernel/Cargo.toml` 无插件依赖；内核/插件入口 `#![forbid(unsafe_code)]` + `#![deny(missing_docs)]`（`crates/kernel/Cargo.toml`、`lib.rs:31-33`）。

---

## 附录 A：全量路由清单（176 method+path）

> 说明：按 `.route(...)` 所在文件分组；`system.rs` 路由挂载在 `/api/system`，CRM 在 `/api/crm`，demo 在 `/api/demo`；行号为 `.route(...)` 起始行。共 128 处 `.route()` 注册，展开 176 个 method+path。

- `crates/kernel/src/app.rs` (3): `GET /metrics` (L95); `GET /api/openapi.json` (L99); `GET /docs` (L103)
- `crates/kernel/src/auth.rs` (7): `POST /api/auth/login` (L539); `POST /api/auth/logout` (L540); `GET /api/auth/me` (L541); `GET /api/auth/tokens` (L542); `POST /api/auth/tokens` (L542); `DELETE /api/auth/tokens/{id}` (L543); `POST /api/auth/change-password` (L544)
- `crates/kernel/src/configs.rs` (4): `GET /api/system/configs` (L92); `POST /api/system/configs` (L92); `GET /api/system/configs/public` (L93); `PUT /api/system/configs/{id}` (L94)
- `crates/kernel/src/exports.rs` (4): `GET /api/system/export-tasks` (L136); `POST /api/system/export-tasks` (L136); `GET /api/system/export-tasks/{id}/download` (L140); `DELETE /api/system/export-tasks/{id}` (L141)
- `crates/kernel/src/files.rs` (5): `GET /api/system/files` (L54); `POST /api/system/files/upload` (L55); `DELETE /api/system/files/{id}` (L56); `GET /api/system/files/{id}` (L56); `GET /api/system/files/{id}/content` (L57)
- `crates/kernel/src/health.rs` (2): `GET /healthz` (L20); `GET /readyz` (L21)
- `crates/kernel/src/notices.rs` (5): `GET /api/system/notices` (L69); `POST /api/system/notices` (L69); `GET /api/system/notices/unread-count` (L70); `POST /api/system/notices/read-all` (L71); `POST /api/system/notices/{id}/read` (L72)
- `crates/kernel/src/posts.rs` (6): `GET /api/system/posts` (L81); `POST /api/system/posts` (L81); `DELETE /api/system/posts/{id}` (L82); `PUT /api/system/posts/{id}` (L82); `GET /api/system/users/{id}/posts` (L86); `PUT /api/system/users/{id}/posts` (L86)
- `crates/kernel/src/system.rs` (93): `GET /api/system/tenants` (L249); `POST /api/system/tenants` (L249); `PUT /api/system/tenants/{id}/status` (L250); `PUT /api/system/tenants/{id}/quota` (L254); `POST /api/system/tenants/purge` (L258); `POST /api/system/tenants/usage/capture` (L259); `GET /api/system/tenants/bills/export` (L263); `GET /api/system/tenants/bills/export.xlsx` (L267); `POST /api/system/tenants/bills/generate` (L271); `GET /api/system/tenants/bills` (L275); `GET /api/system/tenants/bills/reconciliation` (L276); `POST /api/system/tenants/bills/backfill` (L280); `POST /api/system/tenants/bills/overdue/scan` (L284); `POST /api/system/tenants/bills/collections/reminders/scan` (L288); `PUT /api/system/tenants/bills/{id}/status` (L292); `GET /api/system/tenants/bills/{id}/payments` (L296); `POST /api/system/tenants/bills/{id}/payments` (L296); `POST /api/system/tenants/bills/{id}/refunds` (L300); `GET /api/system/tenants/bills/{id}/collections` (L304); `POST /api/system/tenants/bills/{id}/collections` (L304); `PUT /api/system/tenants/{id}/billing` (L308); `GET /api/system/tenants/{id}/bill` (L312); `GET /api/system/tenants/{id}/usage` (L313); `GET /api/system/tenants/{id}/summary` (L314); `DELETE /api/system/tenants/{id}` (L315); `POST /api/system/tenants/{id}/restore` (L316); `GET /api/system/users` (L317); `POST /api/system/users` (L317); `PATCH /api/system/users/{id}/status` (L318); `GET /api/system/users/{id}/roles` (L319); `PUT /api/system/users/{id}/roles` (L319); `POST /api/system/users/{id}/unlock` (L320); `GET /api/system/roles` (L321); `POST /api/system/roles` (L321); `GET /api/system/sessions` (L322); `DELETE /api/system/sessions/{id}` (L323); `GET /api/system/roles/{id}/permissions` (L324); `PUT /api/system/roles/{id}/permissions` (L324); `GET /api/system/roles/{id}/scope-preview` (L328); `GET /api/system/password-policy` (L329); `PUT /api/system/password-policy` (L329); `GET /api/system/roles/{id}/depts` (L333); `PUT /api/system/roles/{id}/depts` (L333); `GET /api/system/permissions` (L334); `GET /api/system/menus` (L335); `POST /api/system/menus` (L335); `GET /api/system/menus/all` (L336); `DELETE /api/system/menus/{id}` (L337); `PUT /api/system/menus/{id}` (L337); `GET /api/system/depts` (L338); `POST /api/system/depts` (L338); `DELETE /api/system/depts/{id}` (L339); `PUT /api/system/depts/{id}` (L339); `GET /api/system/audit-logs` (L340); `GET /api/system/audit-logs/actions` (L341); `GET /api/system/audit-logs/stats` (L342); `GET /api/system/audit-alert-config` (L343); `PUT /api/system/audit-alert-config` (L343); `GET /api/system/audit-alert-rules` (L347); `PUT /api/system/audit-alert-rules` (L347); `POST /api/system/audit-alert-rules/preview` (L351); `GET /api/system/audit-config/export` (L355); `DELETE /api/system/audit-config/snapshots` (L356); `GET /api/system/audit-config/snapshots` (L356); `DELETE /api/system/audit-config/snapshots/{id}` (L360); `PUT /api/system/audit-config/snapshots/{id}` (L360); `GET /api/system/audit-config/snapshots/compare` (L364); `GET /api/system/audit-config/snapshots/{id}/export` (L368); `POST /api/system/audit-config/preview` (L372); `PUT /api/system/audit-config/import` (L376); `GET /api/system/audit-webhook` (L380); `PUT /api/system/audit-webhook` (L380); `GET /api/system/audit-alert-subscriptions` (L384); `PUT /api/system/audit-alert-subscriptions` (L384); `GET /api/system/audit-alert-subscription-config` (L388); `PUT /api/system/audit-alert-subscription-config` (L388); `GET /api/system/audit-alerts/subscribed` (L392); `GET /api/system/audit-alerts` (L393); `GET /api/system/audit-alerts/stats` (L394); `GET /api/system/login-risk` (L395); `GET /api/system/login-policy` (L396); `PUT /api/system/login-policy` (L396); `GET /api/system/audit-delivery-stats` (L400); `GET /api/system/audit-deliveries` (L401); `POST /api/system/audit-deliveries/retry-dead` (L402); `POST /api/system/audit-deliveries/{id}/retry` (L406); `POST /api/system/audit-alerts/{id}/retry` (L410); `POST /api/system/audit-alerts/{id}/ack` (L414); `POST /api/system/imports/{kind}` (L418); `GET /api/system/dicts` (L419); `POST /api/system/dicts` (L419); `GET /api/system/dicts/{code}/items` (L420); `POST /api/system/dicts/{code}/items` (L420)
- `crates/kernel/src/user_import.rs` (3): `POST /api/system/users/import` (L86); `GET /api/system/users/import-template` (L87); `POST /api/system/users/import-errors` (L88)
- `plugins/crm/src/contact.rs` (5): `GET /api/crm/contacts` (L87); `POST /api/crm/contacts` (L87); `DELETE /api/crm/contacts/{id}` (L88); `GET /api/crm/contacts/{id}` (L88); `PATCH /api/crm/contacts/{id}` (L88)
- `plugins/crm/src/contract.rs` (5): `GET /api/crm/contracts` (L104); `POST /api/crm/contracts` (L104); `DELETE /api/crm/contracts/{id}` (L105); `GET /api/crm/contracts/{id}` (L105); `PATCH /api/crm/contracts/{id}` (L105)
- `plugins/crm/src/lib.rs` (19): `GET /api/crm/customers` (L229); `POST /api/crm/customers` (L229); `DELETE /api/crm/customers/{id}` (L230); `GET /api/crm/customers/{id}` (L230); `PATCH /api/crm/customers/{id}` (L230); `GET /api/crm/opportunities/stats` (L234); `GET /api/crm/opportunities/stage-duration` (L235); `GET /api/crm/opportunities/stage-conversion` (L239); `GET /api/crm/opportunities/stuck` (L243); `GET /api/crm/opportunities/{id}/contracts` (L244); `GET /api/crm/opportunities/{id}/stage-history` (L248); `GET /api/crm/dashboard/summary` (L252); `GET /api/crm/dashboard/top-customers` (L253); `GET /api/crm/contracts/{id}/payments` (L254); `GET /api/crm/customers/{id}/contracts` (L255); `GET /api/crm/customers/{id}/contacts` (L256); `GET /api/crm/customers/{id}/opportunities` (L257); `GET /api/crm/customers/export` (L265); `POST /api/crm/customers/import` (L266)
- `plugins/crm/src/opportunity.rs` (5): `GET /api/crm/opportunities` (L90); `POST /api/crm/opportunities` (L90); `DELETE /api/crm/opportunities/{id}` (L91); `GET /api/crm/opportunities/{id}` (L91); `PATCH /api/crm/opportunities/{id}` (L91)
- `plugins/crm/src/payment.rs` (5): `GET /api/crm/payments` (L94); `POST /api/crm/payments` (L94); `DELETE /api/crm/payments/{id}` (L95); `GET /api/crm/payments/{id}` (L95); `PATCH /api/crm/payments/{id}` (L95)
- `plugins/demo/src/lib.rs` (5): `GET /api/demo/ping` (L130); `GET /api/demo/items` (L131); `POST /api/demo/items` (L131); `DELETE /api/demo/items/{id}` (L132); `GET /api/demo/items/{id}` (L132)


## 附录 B：无审计写接口完整清单（27 个）

| # | 文件:行 | 方法路径 | 说明 |
|---|---|---|---|
| 1 | `crates/kernel/src/auth.rs:540` | `POST /api/auth/logout` | 登出无审计 |
| 2 | `crates/kernel/src/auth.rs:542` | `POST /api/auth/tokens` | PAT 创建无审计 |
| 3 | `crates/kernel/src/auth.rs:543` | `DELETE /api/auth/tokens/{id}` | PAT 吊销无审计 |
| 4 | `crates/kernel/src/configs.rs:92` | `POST /api/system/configs` | 配置新增无审计（更新有） |
| 5 | `crates/kernel/src/exports.rs:141` | `DELETE /api/system/export-tasks/{id}` | 导出任务删除无审计 |
| 6 | `crates/kernel/src/files.rs:56` | `DELETE /api/system/files/{id}` | 文件删除无审计 |
| 7 | `crates/kernel/src/notices.rs:71` | `POST /api/system/notices/read-all` | 个人已读，业务上可接受 |
| 8 | `crates/kernel/src/notices.rs:72` | `POST /api/system/notices/{id}/read` | 个人已读 |
| 9 | `crates/kernel/src/posts.rs:86` | `PUT /api/system/users/{id}/posts` | 用户岗位变更无审计 |
| 10 | `crates/kernel/src/system.rs:259` | `POST /api/system/tenants/usage/capture` | 平台维护写无审计 |
| 11 | `crates/kernel/src/system.rs:271` | `POST /api/system/tenants/bills/generate` | 账单生成无审计 |
| 12 | `crates/kernel/src/system.rs:280` | `POST /api/system/tenants/bills/backfill` | 账单补生成无审计 |
| 13 | `crates/kernel/src/system.rs:284` | `POST /api/system/tenants/bills/overdue/scan` | 逾期扫描写告警无审计 |
| 14 | `crates/kernel/src/system.rs:288` | `POST /api/system/tenants/bills/collections/reminders/scan` | 催收扫描写提醒无审计 |
| 15 | `crates/kernel/src/system.rs:321` | `POST /api/system/roles` | 角色创建无审计 |
| 16 | `crates/kernel/src/system.rs:337` | `PUT /api/system/menus/{id}` | 菜单更新无审计 |
| 17 | `crates/kernel/src/system.rs:338` | `POST /api/system/depts` | 部门创建无审计 |
| 18 | `crates/kernel/src/system.rs:351` | `POST /api/system/audit-alert-rules/preview` | 预览，非业务变更 |
| 19 | `crates/kernel/src/system.rs:360` | `PUT /api/system/audit-config/snapshots/{id}` | 审计配置快照更新无审计 |
| 20 | `crates/kernel/src/system.rs:372` | `POST /api/system/audit-config/preview` | 预览，非业务变更 |
| 21 | `crates/kernel/src/system.rs:384` | `PUT /api/system/audit-alert-subscriptions` | 个人订阅，非业务变更 |
| 22 | `crates/kernel/src/system.rs:388` | `PUT /api/system/audit-alert-subscription-config` | 个人 Webhook，非业务变更 |
| 23 | `crates/kernel/src/system.rs:419` | `POST /api/system/dicts` | 字典类型创建无审计 |
| 24 | `crates/kernel/src/system.rs:420` | `POST /api/system/dicts/{code}/items` | 字典项创建无审计 |
| 25 | `crates/kernel/src/user_import.rs:88` | `POST /api/system/users/import-errors` | 纯错误 xlsx 导出，非业务变更 |
| 26 | `plugins/demo/src/lib.rs:131` | `POST /api/demo/items` | demo 写无审计 |
| 27 | `plugins/demo/src/lib.rs:132` | `DELETE /api/demo/items/{id}` | demo 写无审计 |

> 统计：88 个写路由中 **27 个无审计**；扣除 3 个 preview/错误导出（#18、#20、#25）和 2 个个人通知已读（#7、#8）、2 个个人订阅（#21、#22）、1 个登出（#1）后，仍有 **19 个实质业务/安全变更无审计**。`set_role_depts` 通过 `kernel_audit_role_depts` 有审计（`system.rs:2565,2570-2585`），不在本表。

---

## 附录 C：本次扫描的关键统计口径

| 统计 | 数值 | 说明 |
|---|---:|---|
| 生产 Rust 文件 | 44 | `crates/kernel/src` 31 + `plugins/crm/src` 11 + `plugins/demo/src` 1 + `apps/server/src` 1 |
| `.route()` 注册 | 128 | 不含 `apps/server/tests`、`crates/xtask` 的示例/生成器 |
| method+path | 176 | 88 GET / 45 POST / 21 PUT / 16 DELETE / 6 PATCH |
| 写路由 | 88 | 同上 |
| 有权限检查的写路由 | 79 | 89.8% |
| 无权限点的写路由 | 9 | 全为认证/自助 |
| 权限点声明 | 63 | 内核 37 + CRM 23 + demo 3 |
| 实际检查的权限点 | 61 | 0 个检查未声明；2 个死权限点 |
| 有审计的写路由 | 61 | 69.3% |
| 无审计的写路由 | 27 | 见附录 B |
| 业务表总数（内核 + CRM + demo） | 44 | 30 FORCE / 3 ENABLE-only / 11 无 RLS |
| 关联表无 tenant_id | 3 | `sys_user_role`、`sys_role_permission`、`sys_user_post` |
| 超 300 行 Rust 文件 | 17 | 见第六节 |
| `system.rs` SQL 调用 | 198 | `sqlx::query*` 计数 |
