# zcadmin 前端审计报告

- 审计对象：`D:\fcj\zcadmin\frontend`（Vue 3.5 + Vite 6 + TypeScript + Ant Design Vue 4）
- 审计方式：全量静态扫描（40 个手写源文件：28 个 `.vue` + 12 个 `.ts`，不含生成的 `src/types/api.d.ts`），并用 Node 解析 `contracts/openapi.json` 与 kernel/plugin 的 axum 路由做双向比对；另执行 `npm run typecheck`、`npm run build`、仓库自带 `scripts/check-frontend-perms.mjs`。
- 审计结论：**typecheck/build 通过，但规范层面存在系统性缺口**：没有 i18n、没有 vue-query、没有 `@admin/*` 包；28 个文件直接依赖 `ant-design-vue`；10 个页面超过 300 行；路由层没有 `meta.perm` 与 403 页；CI 的按钮权限扫描当前失败。

---

## 1. 量化指标表

| # | 指标 | 实测值 | 证据 |
|---|---|---|---|
| 1 | `.vue` 文件总数 | **28**（26 个页面 + `App.vue` + `UsageTrendChart.vue`） | `Get-ChildItem frontend/src -Recurse -Filter *.vue` |
| 2 | 手写源码规模 | **40 文件 / 9,795 行**（不含 `types/api.d.ts`） | 全量 `ReadAllLines` 统计 |
| 3 | 业务页面路由 | **24** 个页面路由 + `/login` = 25 个可导航路由 | `router.ts:21-108`、`plugins/crm/index.ts:15-66` |
| 4 | 硬编码中文 | **1,145 行 / 4,652 个汉字 / 29 个文件**（已剔除 `//`、`/* */`、`<!-- -->` 注释） | 见 §2.1 全量分文件表；扫描脚本逐行状态机去注释 |
| 5 | vue-i18n | **不存在** | `package.json:14-18` 无依赖；`frontend/node_modules/vue-i18n` 不存在；`package-lock.json` 无条目；`src` 中 `useI18n/$t(/i18n` 0 命中 |
| 6 | @tanstack/vue-query | **不存在** | 同上；`useQuery/useMutation/QueryClient` 0 命中 |
| 7 | pinia | **不存在** | 同上；`defineStore/pinia` 0 命中 |
| 8 | `@admin/sdk` / `@admin/ui` / `@admin/utils` | **均不存在** | `package.json` 无；`node_modules/@admin` 不存在；全仓 `@admin/` 0 命中；无根 `package.json`/workspace |
| 9 | 直接 import `ant-design-vue` 的文件 | **28 个**（25 个 `.vue` + `useImport.ts` + `useExport.ts` + `main.ts`；其中 `HomeView.vue:69` 为 type-only） | 见 §2.2 全量清单 |
| 10 | 手写 `fetch()` 调用 | **6 处**（业务代码 5 处 + 统一客户端 1 处） | `api/client.ts:42`；`CustomersView.vue:247`；`useImport.ts:25`；`FilesView.vue:138`；`UsersView.vue:650,675` |
| 11 | axios | **0**（依赖与源码均无） | `package.json`；全量 grep |
| 12 | 绕过统一 http 的下载入口 | **9 处 `window.open`（其中 8 处直连 API）+ 1 处 `<a href>`** | `CustomersView.vue:237`、`useExport.ts:37`、`AuditLogsView.vue:824`、`ExportsView.vue:151`、`TenantsView.vue:863,867`、`UsersView.vue:370,630`、`FilesView.vue:40` |
| 13 | ESLint | **无** | `frontend/package.json:7-12` 无 `lint`；无 `eslint.config.*`/`.eslintrc*` |
| 14 | Vitest / 任何前端测试 | **无**（0 个 `*.test.*` / `*.spec.*`，无测试脚本、无 vitest 依赖） | 全量文件扫描；`package.json:7-12` |
| 15 | 仓库自带前端权限扫描 | **失败（exit 1）** | `node scripts/check-frontend-perms.mjs` 报 `UsersView.vue: 导出错误` 缺 `v-perm` |
| 16 | 超过 300 行的页面 | **10 个**（最大 1,345 行） | 见 §2.4 |
| 17 | 共享组件 | **仅 1 个**（`components/UsageTrendChart.vue`，114 行） | `frontend/src/components/` 仅此文件 |
| 18 | 重复 CRUD 结构 | `interface PageResult` 13 份、`reload()` 18 份、`onTableChange()` 13 份、`formatAmount()` 8 份、`statusText()` 5 份 | 全量 grep 行号见 §2.6 |
| 19 | `a-table` / `a-empty` | **44 个表格 / 仅 4 个显式空态** | 全量模板扫描；`a-empty` 仅 `HomeView.vue:50`、`LayoutView.vue:48`、`UsersView.vue:166,181` |
| 20 | Ant Design 全局 locale | **未配置**（无 `ConfigProvider`） | 全量 grep `ConfigProvider` 0 命中；`main.ts` 无 locale |
| 21 | `v-perm` 使用 | **87 次 / 18 个文件**；10 个 `.vue` 完全没有 `v-perm` | 全量扫描；见 §2.7 |
| 22 | 路由 `meta.perm` | **0 处**；只有 `meta.public` 1 处 | `router.ts:26` |
| 23 | 403 页面 | **无** | 全量 grep `403/无权限/Forbidden` 仅注释；`router.ts:106` catch-all 直接回 `/` |
| 24 | OpenAPI 契约 | **119 paths / 166 operations** | `contracts/openapi.json` Node 解析 |
| 25 | kernel /api 路由 | **124 条**，其中 **4 条业务接口未进 OpenAPI** | 路由扫描与 OpenAPI 双向比对，见 §3.2 |
| 26 | 前端覆盖的后端操作 | **150 / 166（90.4%）**，16 个操作无前端入口 | 见 §3.3 |
| 27 | typecheck | **通过（exit 0）** | `npm run typecheck`（vue-tsc 2.x） |
| 28 | build | **通过（exit 0），但有 800KB chunk 警告** | `npm run build`；见 §4 |

---

## 2. 规范违规清单（带 file:line）

### 2.1 铁律 12：文案未走 i18n（P0，全量）

- 项目 **没有任何 i18n 基础设施**：`package.json:14-18` 无 `vue-i18n`，`src` 中无 `useI18n` / `$t(` / locale 文件。
- 全量扫描结果：**29 个文件含硬编码中文，共 1,145 行、4,652 个汉字**（注释已剔除）。

| 文件 | 含中文行数 | 首个命中行 |
|---|---:|---|
| `views/AuditLogsView.vue` | 205 | 3 |
| `views/TenantsView.vue` | 145 | 3 |
| `views/UsersView.vue` | 99 | 6 |
| `views/RolesView.vue` | 54 | 3 |
| `plugins/crm/CustomersView.vue` | 48 | 3 |
| `views/SessionsView.vue` | 41 | 3 |
| `plugins/crm/ContractsView.vue` | 40 | 3 |
| `plugins/crm/CustomerDetailView.vue` | 39 | 6 |
| `plugins/crm/OpportunitiesView.vue` | 38 | 3 |
| `plugins/crm/DashboardView.vue` | 36 | 3 |
| `plugins/crm/OpportunityDetailView.vue` | 36 | 6 |
| `plugins/crm/ContactsView.vue` | 34 | 3 |
| `views/DictsView.vue` | 34 | 3 |
| `plugins/crm/PaymentsView.vue` | 34 | 3 |
| `views/MenusView.vue` | 33 | 3 |
| `views/PostsView.vue` | 31 | 3 |
| `views/ConfigsView.vue` | 28 | 3 |
| `views/NoticesView.vue` | 25 | 3 |
| `views/ExportsView.vue` | 23 | 3 |
| `views/HomeView.vue` | 21 | 3 |
| `plugins/crm/ContractDetailView.vue` | 19 | 6 |
| `views/DeptView.vue` | 18 | 3 |
| `views/FilesView.vue` | 18 | 3 |
| `views/LayoutView.vue` | 17 | 44 |
| `plugins/crm/PipelineView.vue` | 12 | 3 |
| `views/LoginView.vue` | 8 | 5 |
| `plugins/crm/useImport.ts` | 4 | 35 |
| `plugins/crm/useExport.ts` | 4 | 36 |
| `api/client.ts` | 1 | 64 |

代表性证据（页面/表单/表格/校验/提示全部硬编码）：

- `views/LoginView.vue:5` `<p class="subtitle">企业级管理平台 · M0</p>`；`:9` `label="用户名"`；`:11` 校验文案；`:36` `登录`。
- `views/UsersView.vue:6` `用户管理`；`:279-288` 10 个表格列标题；`:418-421` 校验文案；`:314` `'加载失败'`。
- `views/TenantsView.vue:3` `title="租户管理"`；`:8-9` 开关文案；`:220-260` 财务动作文案（出账/收款/催收/退款/作废）。
- `plugins/crm/CustomersView.vue:3` `title="客户管理"`；`:161-169` 表格列；`:282-284` 校验；`:312` `'创建成功'`。
- `api/client.ts:64` `` `请求失败（HTTP ${response.status}）` `` —— 连统一错误兜底文案都是中文硬编码。
- 状态枚举直接输出英文原值：`AuditLogsView.vue:126` `{{ record.result }}`、`TenantsView.vue:216` `{{ record.status }}`，中英混排。
- Ant Design 未配置 `ConfigProvider` locale（全量 grep 0 命中），表格空态/分页默认英文（`No Data` / `Page`），与中文页面进一步混排。

### 2.2 铁律 2：业务代码直接 import 组件库（P0，28 个文件）

`package.json:14-18` 只声明 `ant-design-vue`/`vue`/`vue-router`，**没有 `@admin/ui` 之类的封装包**；所有页面直接用 `a-*` 标签并从 `ant-design-vue` 导入 `message`、`Modal`、`FormInstance`、`TablePaginationConfig`。

全量文件:line：

```text
main.ts:38
plugins/crm/ContactsView.vue:103,104          plugins/crm/ContractDetailView.vue:70
plugins/crm/ContractsView.vue:123,124          plugins/crm/CustomerDetailView.vue:120
plugins/crm/CustomersView.vue:130,131          plugins/crm/DashboardView.vue:147
plugins/crm/OpportunitiesView.vue:106,107      plugins/crm/OpportunityDetailView.vue:100,101
plugins/crm/PaymentsView.vue:104,105           plugins/crm/PipelineView.vue:57
plugins/crm/useExport.ts:9                     plugins/crm/useImport.ts:9
views/AuditLogsView.vue:515,516                views/ConfigsView.vue:86,87
views/DeptView.vue:65,66                       views/DictsView.vue:83,84
views/ExportsView.vue:68,69                    views/FilesView.vue:64,65
views/HomeView.vue:69（type-only）              views/LayoutView.vue:115
views/MenusView.vue:99,100                     views/NoticesView.vue:70,71
views/PostsView.vue:91,92                      views/RolesView.vue:132,133
views/SessionsView.vue:103                     views/TenantsView.vue:393
views/UsersView.vue:230,231
```

补充：`main.ts:39` 还直接 `import 'ant-design-vue/dist/reset.css'`。业务页面没有任何 `@admin/ui` 抽象层，组件库升级/换肤会波及全部 26 个页面。

### 2.3 铁律 13：服务端数据未使用 vue-query（P0）

- `@tanstack/vue-query` 不存在（见 §1 #6）。
- **25 个页面**在 `onMounted` 中手工拉数据并写入 `ref`；全量共 **166 处** `http.get/post/put/patch/delete` 调用。
- 典型：`UsersView.vue:300-318`（reload + loading ref）、`TenantsView.vue:638-649`、`CustomersView.vue:191-204`；没有 query key、缓存、失效、重试、并发去重（只有 `stores/session.ts:26-45` 手写了一个 `/me` 去重）。
- `stores/session.ts:24` 把 `/api/auth/me` 的服务端结果放在模块级 `ref`；虽未用 Pinia，但同样不是 vue-query 管理。
- 后果：每次进入页面重复请求、无统一缓存/失效策略；`HomeView.vue:124` 与 `LayoutView.vue:177` 对 `/api/auth/me` 重复请求。

### 2.4 铁律 14：页面组件 > 300 行（P0，10 个）

| 文件 | 总行数 | template + script（不含 style） |
|---|---:|---:|
| `views/AuditLogsView.vue` | **1,345** | 1,299 |
| `views/TenantsView.vue` | **938** | 938 |
| `views/UsersView.vue` | **718** | 705 |
| `views/RolesView.vue` | **421** | 404 |
| `views/LayoutView.vue` | **396** | 302 |
| `plugins/crm/DashboardView.vue` | **372** | 281 |
| `plugins/crm/ContractsView.vue` | **358** | 351 |
| `plugins/crm/CustomersView.vue` | **339** | 336 |
| `plugins/crm/OpportunitiesView.vue` | **321** | 318 |
| `plugins/crm/PaymentsView.vue` | **309** | 306 |

说明：`DashboardView` 若只按 template+script 算是 281 行，但按文件总行数仍超 300；`LayoutView` 两种口径都超。仓库没有 `features/` 目录，超限页面未按铁律拆分。

### 2.5 生成的 API 类型完全未使用（P1）

- `frontend/src/types/api.d.ts` 由 `package.json:12` 的 `gen:api` 生成，共 **10,854 行**。
- 全量 grep：`src` 内 **0 处** import `types/api`；所有页面手写 `interface UserVo/PageResult/...`。
- 后果：OpenAPI 与前端字段漂移不会被 TypeScript 发现；当前已有 4 条接口不在 OpenAPI 中（见 §3.3），但因类型未使用而 typecheck 仍通过。

### 2.6 组件复用与重复 CRUD 逻辑（P1）

- 共享组件只有 `components/UsageTrendChart.vue`（114 行）；其余 26 个页面直接堆模板。
- CRM 5 个列表页（`ContactsView` 297 行、`ContractsView` 358、`CustomersView` 339、`OpportunitiesView` 321、`PaymentsView` 309，合计 **1,624 行**）结构几乎相同：`PageResult` + `reload()` + `onTableChange()` + `a-modal` 表单 + `a-popconfirm` 删除。
- 重复定义：
  - `interface PageResult`：13 处（`ContactsView.vue:123`、`ContractsView.vue:150`、`CustomersView.vue:149`、`OpportunitiesView.vue:125`、`PaymentsView.vue:125`、`useExport.ts:18`、`AuditLogsView.vue:585`、`ConfigsView.vue:100`、`ExportsView.vue:85`、`FilesView.vue:83`、`NoticesView.vue:83`、`PostsView.vue:104`、`UsersView.vue:252`）。
  - `async function reload()`：18 处（行号见 `ContactsView:163`、`ContractsView:212`、`CustomersView:191`、`DashboardView:255`、`OpportunitiesView:183`、`PaymentsView:183`、`PipelineView:108`、`AuditLogsView:779`、`ConfigsView:121`、`DeptView:83`、`ExportsView:124`、`FilesView:109`、`MenusView:152`、`NoticesView:116`、`PostsView:130`、`TenantsView:638`、`UsersView:300`）。
  - `function onTableChange()`：13 处（`ContactsView:184`、`ContractDetailView:146`、`ContractsView:238`、`CustomersView:206`、`OpportunitiesView:206`、`PaymentsView:208`、`AuditLogsView:840`、`ConfigsView:138`、`ExportsView:139`、`FilesView:124`、`NoticesView:131`、`PostsView:145`、`UsersView:320`）。
  - `formatAmount()`：8 处（`ContractDetailView:132`、`ContractsView:208`、`CustomerDetailView:216`、`DashboardView:251`、`OpportunitiesView:178`、`OpportunityDetailView:173`、`PaymentsView:174`、`PipelineView:104`）。
  - `statusText()`：5 处（`ContractDetailView:123`、`ContractsView:199`、`CustomersView:181`、`ExportsView:113`、`UsersView:291`）。
- `composables/useDict.ts` 实现完整但 **0 个页面调用**；`clearDictCache()` 从未被调用（全仓仅定义处 `useDict.ts:49`）。

### 2.7 运行时组件未注册（P1，7 个组件）

`main.ts:13-38` 只注册了 Alert/Badge/Button/Card/Checkbox/Descriptions/Divider/Drawer/Empty/Form/Input/InputNumber/Layout/Menu/Modal/Popconfirm/Popover/Select/Space/Spin/Switch/Table/Tag/Upload，`main.ts:49-74` 再次使用同一列表。以下标签在模板中使用但**没有任何注册**，运行时 Vue 会报 `Failed to resolve component`，控件不渲染（构建不会报错）：

| 未注册组件 | 使用位置 |
|---|---|
| `a-row` | `HomeView.vue:28,37`、`DashboardView.vue:9`、`CustomerDetailView.vue:12`、`PipelineView.vue:8` |
| `a-col` | `HomeView.vue:29-32,38,48`、`DashboardView.vue:10,17,24,31`、`CustomerDetailView.vue:13,20,26,32`、`PipelineView.vue:9,15,21` |
| `a-statistic` | `HomeView.vue:29-32`、`AuditLogsView.vue:64,66,67,68,72,75,76,77` |
| `a-progress` | `ContractDetailView.vue:28`、`TenantsView.vue:48,57` |
| `a-radio-group` | `HomeView.vue:22`、`TenantsView.vue:306` |
| `a-radio-button` | `HomeView.vue:23-25`、`TenantsView.vue:307-310` |
| `a-range-picker` | `PaymentsView.vue:10`、`AuditLogsView.vue:23` |

影响：工作台/看板/客户详情的 KPI 栅格、统计卡、进度条、趋势切换、日期范围筛选在运行时不可用。

### 2.8 按钮级权限遗漏（P1）

全量：`v-perm` 87 次，分布在 18 个 `.vue`；**10 个 `.vue` 完全没有 `v-perm`**（`ContractDetailView`、`CustomerDetailView`、`DashboardView`、`PipelineView`、`ExportsView`、`HomeView`、`LayoutView`、`LoginView`、`App.vue`、`UsageTrendChart.vue`）。

全量扫描共 **43 个 `<a-button>` 未带 `v-perm`**；剔除返回/刷新/取消/登录/复制/通知等约 14 个中性按钮后，仍有约 29 个业务入口没有按钮级权限。重点：

- `views/UsersView.vue:63`（分配角色）、`:66`（岗位）——无 `v-perm`；`:187`（导出错误）——无 `v-perm`，且 **CI 权限扫描当前因它失败**。
- `views/RolesView.vue:19`（授权）、`:22`（数据范围）、`:23`（指定部门）——无 `v-perm`；`:102`（保存授权）也无二次确认。
- `views/SessionsView.vue:32`（新建令牌）、`:62`（吊销令牌）——无 `v-perm`（脚本允许列表把个人令牌放行，但页面本身无路由权限）。
- `views/ExportsView.vue:35-46`（下载/详情/删除）——整个文件 0 个 `v-perm`，删除仅 popconfirm。
- `views/TenantsView.vue:220-260`（出账/收款/催收/退款/作废）——5 个财务动作全部无 `v-perm`；`:252-260` 的“作废”无二次确认。
- `views/AuditLogsView.vue:55,56`（订阅/我的订阅告警）、`:154`（订阅告警处理）、`:416`（规则删除）、`:422,435`（规则测试）——无 `v-perm`；规则编辑器行内删除被 `scripts/check-frontend-perms.mjs:17` 的允许列表放行。
- `views/DictsView.vue:16`（字典项入口）——无 `v-perm`。

`scripts/check-frontend-perms.mjs` 只按“新建/新增/编辑/删除/导出/导入/上传/回滚/命名/清理/重投/强制/吊销”关键词匹配，覆盖不到“授权/分配角色/收款/退款/作废/出账”等动作；且 `:14-19` 的 allowlist 明确放行了 `SessionsView.vue:新建令牌`、`SessionsView.vue:吊销`、`AuditLogsView.vue:删除`、`ExportsView.vue:删除`。

### 2.9 工程化缺口（P1）

- 无 ESLint 配置、无 lint 脚本：`frontend/package.json:7-12`。
- 无 Vitest/单测/E2E：0 个 `*.test.*` / `*.spec.*`，无测试依赖/脚本。
- CI 在 `.github/workflows/ci.yml:68-69` 调用 `node scripts/check-frontend-perms.mjs`；本地实跑 **exit 1**，即当前 CI 会红。CI 前端 job 只有 `npm ci` + `typecheck` + `build`（`.github/workflows/ci.yml:160-182`），没有 lint/test。
- 合同检查 `.github/workflows/ci.yml:77-81` 只校验 `contracts/openapi.json` 与代码生成结果一致，不校验“所有 kernel 路由是否登记”，因此 4 条漏登记接口不会被发现（见 §3.3）。

---

## 3. 页面-接口缺口表

### 3.1 路由、页面与接口覆盖（全量 25 个路由）

> 说明：`router.ts` 为系统路由，`plugins/crm/index.ts` 为 CRM 插件路由；`—` 表示该页面不适用。

| 路由 | 页面文件 | 已接后端接口（摘要） | 缺口 |
|---|---|---|---|
| `/login` | `views/LoginView.vue` | `POST /api/auth/login` | 无 |
| `/` | `views/HomeView.vue` | `GET /api/auth/me`、`GET /api/demo/ping`、`GET /api/system/login-risk` | 无 loading；3 个静默 catch；`/api/demo/ping` 与 `login-risk` 不在 OpenAPI |
| `/system/users` | `views/UsersView.vue` | `GET/POST /api/system/users`、`/users/{id}/roles` GET/PUT、`/users/{id}/posts` GET/PUT、`/users/{id}/unlock`、`/password-policy`、`/login-policy`、`/export-tasks`、`/users/import`、`/users/import-template`、`/users/import-errors` | **`PATCH /api/system/users/{id}/status` 无入口**（不能禁用/锁定用户）；无编辑/删除；`GET/PUT login-policy` 不在 OpenAPI |
| `/system/depts` | `views/DeptView.vue` | `GET/POST /api/system/depts`、`PUT/DELETE /api/system/depts/{id}` | 无 |
| `/system/audit` | `views/AuditLogsView.vue` | `/audit-logs`、`/audit-logs/actions`、`/audit-logs/stats`、`/audit-alert-config`、`/audit-alert-rules`、`/audit-alert-rules/preview`、`/audit-alert-subscription-config`、`/audit-alert-subscriptions`、`/audit-alerts`、`/audit-alerts/stats`、`/audit-alerts/subscribed`、`/audit-alerts/{id}/ack`、`/audit-alerts/{id}/retry`、`/audit-deliveries`、`/audit-deliveries/retry-dead`、`/audit-deliveries/{id}/retry`、`/audit-delivery-stats`、`/audit-config/*`（export/import/preview/snapshots/compare）、`/audit-webhook` | 无接口级缺口；但多处敏感动作无 `v-perm`/确认（见 §2.8） |
| `/system/sessions` | `views/SessionsView.vue` | `GET /api/system/sessions`、`DELETE /api/system/sessions/{id}`、`GET/POST /api/auth/tokens`、`DELETE /api/auth/tokens/{id}` | 无 |
| `/system/configs` | `views/ConfigsView.vue` | `GET/POST /api/system/configs`、`PUT /api/system/configs/{id}` | `GET /api/system/configs/public` 无任何前端消费；无删除（后端无接口） |
| `/system/roles` | `views/RolesView.vue` | `GET/POST /api/system/roles`、`/permissions`、`/roles/{id}/permissions` GET/PUT、`/roles/{id}/depts` GET/PUT、`/roles/{id}/scope-preview` | **无编辑角色（名称/描述/数据范围）、无删除**（后端也没有 update/delete 路由，`system.rs:321`） |
| `/system/dicts` | `views/DictsView.vue` | `GET/POST /api/system/dicts`、`GET/POST /api/system/dicts/{code}/items` | 只有“新增”，无编辑/删除/排序（后端也无 update/delete 路由，`system.rs:419-423`） |
| `/system/files` | `views/FilesView.vue` | `GET /api/system/files`、`POST /api/system/files/upload`、`DELETE /api/system/files/{id}`、`GET /api/system/files/{id}/content`（`<a href>`） | `GET /api/system/files/{id}` 元数据无入口；无预览/重命名 |
| `/system/notices` | `views/NoticesView.vue` | `GET/POST /api/system/notices` | 无编辑/删除（后端无接口）；列表有分页 |
| `/system/menus` | `views/MenusView.vue` | `GET /api/system/menus/all`、`POST/PUT/DELETE /api/system/menus` | 无 |
| `/system/exports` | `views/ExportsView.vue` | `GET /api/system/export-tasks`、`DELETE /api/system/export-tasks/{id}`、`GET .../{id}/download` | 无独立创建入口（由各业务页触发，设计如此）；全页无 `v-perm` |
| `/system/tenants` | `views/TenantsView.vue` | `GET/POST /api/system/tenants`、`DELETE /{id}`、`POST /{id}/restore`、`PUT /{id}/status`、`PUT /{id}/quota`、`PUT /{id}/billing`、`GET /{id}/summary`、`GET /{id}/usage`、`GET /{id}/bill`、`GET /tenants/bills`、`GET /bills/reconciliation`、`GET /bills/export(.xlsx)`、`PUT /bills/{id}/status`、`POST /bills/{id}/payments`、`POST /bills/{id}/refunds`、`GET/POST /bills/{id}/collections` | **5 个批量任务无入口**：`POST /tenants/bills/generate`、`/bills/backfill`、`/bills/overdue/scan`、`/bills/collections/reminders/scan`、`/tenants/usage/capture`；`GET /tenants/bills/{id}/payments` 无入口；无服务端分页 |
| `/system/posts` | `views/PostsView.vue` | `GET/POST /api/system/posts`、`PUT/DELETE /api/system/posts/{id}` | 无 |
| `/crm/dashboard` | `plugins/crm/DashboardView.vue` | `GET /crm/dashboard/summary`、`/crm/dashboard/top-customers`、`/crm/opportunities/stage-duration`、`/crm/opportunities/stage-conversion`、`/crm/opportunities/stuck` | `top-customers` 不在 OpenAPI；`a-row/a-col` 未注册 |
| `/crm/customers` | `plugins/crm/CustomersView.vue` | `GET/POST /api/crm/customers`、`DELETE /{id}`、`GET /customers/export`、`POST /customers/import` | **`PATCH /api/crm/customers/{id}` 无入口（无编辑）**；列表无编辑按钮 |
| `/crm/customers/:id` | `plugins/crm/CustomerDetailView.vue` | `GET /crm/customers/{id}`、`/customers/{id}/contacts`、`/opportunities`、`/contracts` | 只读；无编辑；404/空数据无 `a-empty`，页面空白 |
| `/crm/contacts` | `plugins/crm/ContactsView.vue` | `GET/POST /api/crm/contacts`、`PATCH/DELETE /api/crm/contacts/{id}`、`POST /api/system/imports/crm.contacts`、`POST /api/system/export-tasks` | `GET /api/crm/contacts/{id}` 未用（列表/编辑弹窗已覆盖数据） |
| `/crm/opportunities` | `plugins/crm/OpportunitiesView.vue` | `GET/POST /api/crm/opportunities`、`PATCH/DELETE /{id}`、`GET /api/crm/customers`、导入/导出 | 无 |
| `/crm/opportunities/:id` | `plugins/crm/OpportunityDetailView.vue` | `GET /opportunities/{id}`、`/{id}/contracts`、`/{id}/stage-history`、`POST /api/crm/contracts` | 无 |
| `/crm/pipeline` | `plugins/crm/PipelineView.vue` | `GET /api/crm/opportunities/stats` | 只读（设计如此）；`a-row/a-col` 未注册 |
| `/crm/contracts` | `plugins/crm/ContractsView.vue` | `GET/POST /api/crm/contracts`、`PATCH/DELETE /{id}`、客户/商机下拉、导入/导出 | 无 |
| `/crm/contracts/:id` | `plugins/crm/ContractDetailView.vue` | `GET /contracts/{id}`、`GET /contracts/{id}/payments` | 只读（设计如此）；`a-progress` 未注册 |
| `/crm/payments` | `plugins/crm/PaymentsView.vue` | `GET/POST /api/crm/payments`、`PATCH/DELETE /{id}`、合同下拉、导入/导出 | `GET /api/crm/payments/{id}` 未用；`a-range-picker` 未注册 |

### 3.2 后端有接口、前端无页面/无入口（16 个操作）

| 后端接口 | 证据 | 现状 |
|---|---|---|
| `GET /api/demo/items`、`POST /api/demo/items`、`GET /api/demo/items/{id}`、`DELETE /api/demo/items/{id}` | `plugins/demo/src/lib.rs:130-132`；`frontend/src/plugins/index.ts:16` 只注册 `crmPlugin` | **整个 demo 模块无前端页面**，仅 Home 调用 `/api/demo/ping` 探活 |
| `POST /api/system/tenants/bills/generate` | `system.rs:271-274` | 无“生成账单”入口 |
| `POST /api/system/tenants/bills/backfill` | `system.rs:280-283` | 无入口 |
| `POST /api/system/tenants/bills/overdue/scan` | `system.rs:284-287` | 无入口 |
| `POST /api/system/tenants/bills/collections/reminders/scan` | `system.rs:288-291` | 无入口 |
| `POST /api/system/tenants/usage/capture` | `system.rs:259-262` | 无入口 |
| `PATCH /api/system/users/{id}/status` | `system.rs:318` | 无入口；`UsersView.vue:61-70` 操作列只有分配角色/岗位/解锁 |
| `PATCH /api/crm/customers/{id}` | `plugins/crm/src/lib.rs:230` | 无入口；`CustomersView.vue:54-60` 只有删除，`CustomerDetailView.vue:40-48` 只读 |
| `GET /api/crm/payments/{id}` | `plugins/crm/src/lib.rs` payment route | 无入口（列表/编辑弹窗已覆盖） |
| `GET /api/system/tenants/bills/{id}/payments` | `system.rs:296-299` | 无入口；Tenants 只调 collections |
| `GET /api/system/configs/public` | `configs.rs:93` | 无任何前端消费 |
| `GET /api/system/files/{id}` | `files.rs:56` | 无元数据详情入口 |
| `GET /api/crm/contacts/{id}` | `plugins/crm/src/contact.rs:88` | 无详情页（列表内编辑） |

### 3.3 前端调用但不在 OpenAPI 契约（4 条路径，5 个操作）

> 这些接口在 kernel/plugin 路由中**真实存在**，不是“调用不存在的接口”；问题是 `contracts/openapi.json` 漏登记，导致契约/生成类型不完整。

| 前端调用 | 前端 file:line | kernel 实现 | 影响 |
|---|---|---|---|
| `GET /api/crm/dashboard/top-customers` | `DashboardView.vue:260` | `plugins/crm/src/lib.rs:253` | OpenAPI 无；generated types 无 |
| `GET /api/demo/ping` | `HomeView.vue:135` | `plugins/demo/src/lib.rs:130` | OpenAPI 无 |
| `GET /api/system/login-risk` | `HomeView.vue:148` | `crates/kernel/src/system.rs:395` | OpenAPI 无 |
| `GET /api/system/login-policy` | `UsersView.vue:338` | `crates/kernel/src/system.rs:396-399` | OpenAPI 无 |
| `PUT /api/system/login-policy` | `UsersView.vue:348` | 同上 | OpenAPI 无 |

根因证据：`system.rs` 的 `SystemApi` `paths(...)` 列表（自 `system.rs:2428` 起）未包含 `login_risk/get_login_policy/update_login_policy`；`crates/kernel/src/openapi.rs` 只合并各模块片段，不做“路由表 vs 片段”完整性校验。

### 3.4 路由级权限与菜单一致性

**路由权限（P0）**

- `router.ts` **没有任何 `meta.perm`**：全文只有 `router.ts:26` 的 `meta: { public: true }`。
- 守卫 `router.ts:116-127` 只做 `loadSession()`，只要会话有效就 `return true`；没有 `hasPerm`、没有按页面权限拦截。
- **没有 403 页面**；`router.ts:106` 的 `/:pathMatch(.*)*` 直接 `redirect: '/'`，无权/不存在路径都静默回首页。
- 插件路由对所有登录用户注册：`router.ts:103` `...frontendPlugins.flatMap((plugin) => plugin.routes)`；`plugins/types.ts:16-21` 的 `FrontendPlugin` 只有 `id/routes`，没有权限字段；`plugins/crm/index.ts:15-66` 的 10 条路由均无 `meta`。
- 结果：任何登录用户可直接输入 `/system/tenants`、`/system/audit`、`/crm/payments` 等 URL；页面会渲染并发出请求，后端返回 403 后只弹 toast，没有统一 403 页/跳转。前端 `v-perm` 只移除按钮 DOM（`directives/perm.ts:16-22`），不是路由或安全边界。

**菜单与路由一致性**

- 后端默认菜单共 **21 个叶子**：系统 13 个（`system.rs:124-219` 的 `ensure_default_menus` + `0024_session_permissions.sql:10-16` 的 `/system/sessions`）+ 工作台 `/` + CRM 7 个（`plugins/crm/src/lib.rs:159-217`）。
- 前端对应导航路由也是 21 个（13 系统 + 首页 + 7 CRM），路径逐条一致：`/`、`/system/users`、`/system/roles`、`/system/dicts`、`/system/files`、`/system/notices`、`/system/depts`、`/system/audit`、`/system/tenants`、`/system/configs`、`/system/menus`、`/system/exports`、`/system/posts`、`/system/sessions`、`/crm/dashboard`、`/crm/customers`、`/crm/contacts`、`/crm/opportunities`、`/crm/pipeline`、`/crm/contracts`、`/crm/payments`。
- 3 个详情路由（`/crm/customers/:id`、`/crm/opportunities/:id`、`/crm/contracts/:id`）不在菜单中，属正常设计。
- **菜单不携带权限信息**：`MenuNode` 只有 `id/title/path/icon/children`（`system.rs:1369-1381`），没有 `perm`；前端无法从菜单树反推路由权限。后端仅在下发前按 `perm` 过滤（`system.rs:1412-1419`）。
- **新租户菜单漂移**：`create_tenant` 会调用 `ensure_default_menus`（`system.rs:5649`），但该函数不包含 `/system/sessions`；`0024_session_permissions.sql` 只给迁移时已存在的租户插入 sessions 菜单；插件菜单 `sync_plugin_menus` 只在启动时执行（`app.rs:70-71`，实现 `system.rs:1977-2017`）。因此**运行期新建的租户在重启前没有“在线会话”和全部 CRM 菜单**（页面 URL 仍可直达）。
- 菜单排序有重复值：审计 `/system/audit` 与 sessions 都是 sort 17（`system.rs:180`、`0024...:11`），租户与菜单都是 19（`system.rs:187,201`），菜单顺序不稳定。

### 3.5 “只有列表没有增删改”清单

| 页面 | 现有能力 | 缺失 |
|---|---|---|
| `/system/users` | 列表、新建、解锁、分配角色/岗位、导入/导出 | **编辑用户**、**启用/禁用/锁定（后端有 PATCH status）**、删除（后端无） |
| `/system/roles` | 列表、新建、授权、指定部门、数据范围预览 | **编辑角色名称/描述/数据范围**、删除（后端无 update/delete 路由） |
| `/system/dicts` | 字典类型/字典项均为“列表 + 新增” | 编辑、删除、停用、排序（后端无接口） |
| `/system/notices` | 列表、发布 | 编辑、删除、撤回（后端无接口） |
| `/system/configs` | 列表、新增、编辑 | 删除（后端无接口） |
| `/system/exports` | 列表、下载、删除 | 无独立新建（业务页触发，设计如此） |
| `/crm/customers` | 列表、新建、删除、导入/导出 | **编辑（后端 `PATCH /api/crm/customers/{id}` 存在但前端未接）** |
| `/crm/customers/:id` | 详情 + 联系人/商机/合同只读 | 编辑客户、快捷新建联系人/商机 |
| `/crm/payments` | 列表、新建、编辑、删除、导入/导出 | 详情接口未接（非必需） |
| `/crm/contacts`、`/crm/contracts`、`/crm/opportunities` | 列表、新建、编辑、删除、导入/导出 | 完整 |

### 3.6 交互完整性抽查（用户/角色/租户/CRM 客户详情）

| 页面 | loading | 错误处理 | 空态 | 分页 | 二次确认 |
|---|---|---|---|---|---|
| `UsersView.vue` | ✅ `:loading="loading"`（`:42`）、导出/导入 loading | ✅ `message.error`（`:314` 等 14 处） | ⚠️ 表格默认空态；仅角色/岗位弹窗有 `a-empty`（`:166,181`） | ✅ 服务端分页（`:44-50,320-324`） | ✅ 解锁 popconfirm（`:67`）；❌ 分配角色/岗位无确认 |
| `RolesView.vue` | ✅ `:loading`（`:8`）、授权/预览 spin | ✅ `message.error`（7 处） | ⚠️ 表格默认空态 | ❌ 无分页（角色少可接受） | ❌ 保存授权（`:102`）无二次确认 |
| `TenantsView.vue` | ✅ `:loading`（`:26`）、详情/账单/对账 spin | ✅ `message.error`（15 处） | ⚠️ 表格默认空态 | ⚠️ 无服务端分页；表格默认客户端 10 条 | ✅ 删除/停用/清理 popconfirm（`:16,98,106`）；❌ 账单“出账/作废”（`:220-260`）无确认 |
| `CustomerDetailView.vue` | ✅ `a-spin`（`:11`） | ✅ `message.error`（`:235`） | ❌ 404/空数据无 `a-empty`，卡片空白 | — 内嵌列表 `pagination=false`（设计如此） | — 无破坏性操作；**无编辑入口** |
| `CustomersView.vue`（CRM 列表） | ✅ `:loading`（`:31`） | ✅ `message.error`（4 处） | ⚠️ 表格默认空态 | ✅ 服务端分页（`:33-40,206-210`） | ✅ 删除 popconfirm（`:55`）；无编辑按钮 |
| `AuditLogsView.vue` | ✅ 多个 `:loading`/spin | ✅ `message.error`（24 处） | ⚠️ 表格默认空态；少数弹窗有文字空态 | ✅ 主表/快照服务端分页（`:114-121,286`） | ✅ 快照清理/删除/回滚有确认；❌ 规则保存/订阅保存无确认 |

补充：全量 44 个 `a-table`，仅 4 个 `a-empty`；无 `ConfigProvider`，空态默认英文；30 个 `catch` 块中有 **10 个完全静默**（`ContactsView.vue:232`、`ContractsView.vue:289`、`OpportunitiesView.vue:250`、`OpportunityDetailView.vue:236`、`PaymentsView.vue:252`、`LayoutView.vue:251,276,286`、`UsersView.vue:485,493`），另有 `HomeView.vue:130,137,149` 仅改标志位不提示错误。

---

## 4. 构建结果

环境：Node `v24.11.0`，npm `11.6.1`，vite `6.4.5`，`vue-tsc`（`npm run typecheck`）。

### 4.1 `npm run typecheck`

```text
> zcadmin-web@0.1.0 typecheck
> vue-tsc --noEmit

EXIT_CODE=0
```

**通过，无类型错误。** 注意：这不能证明契约正确——生成的 `src/types/api.d.ts` 未被任何代码 import（§2.5），且 4 条接口不在 OpenAPI（§3.3）也不会触发类型错误。

### 4.2 `npm run build`

```text
vite v6.4.5 building for production...
✓ 3266 modules transformed.
✓ built in 15.20s
EXIT_CODE=0
```

产物：

- `frontend/dist` 共 **59 个文件 / 1.18 MB**。
- 最大 chunk：`dist/assets/antd-D2Q4uCgG.js` **908.22 kB（gzip 273.83 kB）**。
- 第二：`dist/assets/vue-moy5ooXg.js` 105.93 kB（gzip 41.17 kB）。
- 业务页最大：`AuditLogsView` 32.80 kB、`TenantsView` 24.36 kB、`UsersView` 16.22 kB。

**警告（已超 800KB 阈值）**：

```text
(!) Some chunks are larger than 800 kB after minification. Consider:
- Using dynamic import() to code-split the application
- Use build.rollupOptions.output.manualChunks to improve chunking
- Adjust chunk size limit for this warning via build.chunkSizeWarningLimit.
```

`vite.config.ts:35` 设置 `chunkSizeWarningLimit: 800`；`vite.config.ts:40-43` 的 `manualChunks` 把整个 `ant-design-vue` 打成单一 `antd` chunk，实际 908.22 kB，**超过 800KB chunk 限制**。虽然 `main.ts` 声称“按需注册”，但 Rollup 仍把组件库整体拉入该 chunk。

### 4.3 其他只读校验

```text
node scripts/check-frontend-perms.mjs
EXIT=1
缺少 v-perm 的按钮:
  - frontend\src\views\UsersView.vue: 导出错误
```

```text
node scripts/check-perm-codes.mjs        -> 权限码校验通过（声明 63 个），EXIT=0
node scripts/gen-perm-codes.mjs --check  -> 权限常量已是最新，EXIT=0
```

结论：**typecheck/build 绿，但仓库自带的前端按钮权限扫描红**；无 ESLint、无 vitest、无任何前端测试。CI 的 backend job 会因 `check-frontend-perms.mjs` 失败（`.github/workflows/ci.yml:68-69`）。

---

## 5. 主要风险排序（给修复排期）

| 优先级 | 问题 | 证据 | 建议 |
|---|---|---|---|
| P0 | 无 i18n，1,145 行硬编码中文 | §2.1 | 引入 `vue-i18n`，先抽页面标题/表格列/校验/消息四类文案 |
| P0 | 业务代码直接依赖 `ant-design-vue`，无 `@admin/ui` | §2.2（28 文件） | 建 `@admin/ui` 封装 message/Modal/Table/Form，页面只 import 封装层 |
| P0 | 服务端数据全部手写 `onMounted + ref`，无 vue-query | §2.3（25 页面/166 调用） | 建 query keys + `useQuery/useMutation`，统一失效/重试/错误 |
| P0 | 路由无 `meta.perm`、无 403 页，插件路由对所有登录用户开放 | §3.4 | 给 24 条路由加 `meta.perm`，守卫 `hasPerm` 失败跳 403；403 页新增 |
| P0 | 10 个页面 >300 行（最大 1,345） | §2.4 | 按 `features/` 拆表格、表单、工具栏；优先 Audit/Tenants/Users |
| P1 | 7 个 Ant 组件未注册，运行时控件不渲染 | §2.7 | `main.ts` 注册 Row/Col/Statistic/Progress/Radio/DatePicker（或改用已封装组件） |
| P1 | CI 按钮权限扫描失败；29 个业务按钮无 `v-perm` | §2.8 | 给授权/财务/会话/导出动作补 `v-perm`；扩展扫描关键词 |
| P1 | 生成 API 类型未使用；4 条接口不在 OpenAPI | §2.5/§3.3 | 页面改用 `paths[...]` 类型；把 login-risk/login-policy/top-customers/demo ping 加入对应 `paths(...)` |
| P1 | 无 ESLint/Vitest/前端测试 | §2.9 | 加 `eslint` + `vitest`，CI 增加 lint/test |
| P1 | antd chunk 908KB > 800KB 警告 | §4.2 | 按路由/组件拆 chunk，检查 `manualChunks` 是否应改为函数式拆分 |
| P2 | CRUD 重复逻辑、仅 1 个共享组件 | §2.6 | 抽 `useCrudTable`/`CrudTable`/`CrudModal`；先合并 5 个 CRM 列表页 |
| P2 | 空态/错误处理不统一，10 个完全静默 catch | §3.6 | 统一 `AppEmpty`/`AppError`；禁止空 catch |
| P2 | 新租户缺 sessions/CRM 菜单 | §3.4 | `ensure_default_menus` 补 sessions；`create_tenant` 后调用 `sync_plugin_menus` |

---

### 附：审计口径说明

1. 硬编码中文统计使用逐行状态机剔除 `//`、`/* ... */`、`<!-- ... -->` 注释；`types/api.d.ts` 为生成文件，其中中文均在注释内，未计入。统计的是“含中文的代码行”，同一行多个中文按 1 行计；汉字数为实际 Unicode CJK 字符数。
2. 路由/接口比对使用 Node 解析 `contracts/openapi.json`（119 paths / 166 operations），并与 `crates/kernel/src/*.rs`、`plugins/*/src/*.rs` 的 `.route("...")` 做前缀还原（`/api/system`、`/api/crm`、`/api/demo`）后比对；动态段统一归一为 `{id}`。
3. 前端调用统计覆盖 `http.get/post/put/patch/delete`、`fetch()`、`window.open`、`<a href>`；未把 `types/api.d.ts` 的字符串算作调用。
4. 本报告未修改任何项目代码；仅执行只读校验与 `npm run build`（其产物写入 `frontend/dist`）。
