# zcadmin 业务域与数据正确性审计

- 仓库：`D:\fcj\zcadmin`（Windows / Rust axum + sqlx / PostgreSQL）
- 范围：`plugins/crm`、`crates/kernel/src`（system.rs / exports.rs / imports.rs / user_import.rs / tenant_usage.rs / tenant.rs / scope.rs / posts.rs / audit.rs / jobs.rs / app.rs）、`crates/kernel/migrations/0055~0066`、`crates/xtask/src/gen.rs`、`frontend/src/plugins/crm`
- 方法：**全量通读**上述文件（非抽样），用 `Select-String` 对全仓做交叉核对（`REFERENCES|UNIQUE|CHECK|RLS|%all%|record_user|take(2000)|LIMIT 20000|overdue_tier` 等），并逐条比对 `plugins/crm/CONTEXT.md` 与 `docs/03/05/06/07` 的规格描述。
- 结论口径：完成度按「规格条目已可用且口径正确」计，**不按代码行数**；任何会导致财务/业务数字错误的缺陷一律不低于 P1，会让用户看到错数或让数据无法回滚的一律 P0。

> 一句话结论：CRM 的**单表 CRUD 骨架质量不错**（软删、部分唯一索引、RLS FORCE、TenantTx、审计快照都在），但**领域语义层（状态机/事件/公海/跟进/退款）几乎空白**，且**预聚合与软删、导入事务、Excel 表头三处存在会产生错误数字的硬缺陷**；租户计费的「月末自动生成」实际不生效，账单生成会覆盖已出账/已收款账单。

---

## 一、各业务域完成度表

### 1.1 CRM 业务闭环（对照 `plugins/crm/CONTEXT.md` + `docs/06` §8.1 + `docs/07` §6.2）

| # | 业务域 | 完成度 | 已实现（证据） | 缺口（证据） |
|---|---|---|---|---|
| 1 | 客户 Customer | **85%** | 表与约束齐全：`(tenant_id,code)` 部分唯一 + status CHECK（`plugins/crm/migrations/0001_init.sql:29-31,48-50`）；CRUD+分页+keyword/level/status 过滤（`plugins/crm/src/api.rs:32-348`）；软删（`api.rs:305-348`）；越权=404（`api.rs:141-142`）；审计 before/after 整行快照（`api.rs:237,281-291`）；RLS 策略+FORCE（`0003_rls.sql:8-18`、`0004_force_rls.sql:7-8`） | 无「转移负责人」接口 → `crm.customer.transferred` 事件无从产生（`lib.rs:159-218` 无该路由）；`UpdateCustomerRequest` 无 `ownerId/deptId`（`model.rs:80-93`）；状态机仅做集合校验，`signed → potential` 可直达（`api.rs:238-242` vs `CONTEXT.md:76`）；无乐观锁（`version+1` 但无 `WHERE version=`，`api.rs:255`）；`level` 未校验字典（`api.rs:190`） |
| 2 | 联系人 Contact | **55%** | 五接口齐全（`contact.rs:85-89`）；软删+审计（`contact.rs:403-463`）；`is_primary` 参与排序（`api.rs:400`） | **创建/更新不校验 `customer_id` 是否可见**（`contact.rs:268-307`、`318-393`），可挂到别部门/别租户客户；`is_primary` 无唯一约束，可多个主联系人（`0002_create_contact.sql:14` vs `docs/07:252`「最多 1 个主联系人」）；设主联系人不自动降级旧主；详情用 `contact:list` 权限，无 `contact:get`（`contact.rs:217`，`lib.rs:91-106` 未声明该权限点）；无联系人导出/导入独立权限点 |
| 3 | 商机 Opportunity | **50%** | 五接口 + 客户维度列表（`opportunity.rs:88-95`、`api.rs:419-441`）；`contract.opportunity_id` 反查合同（`0015_contract_opportunity.sql:8-14`、`api.rs:850-892`）；漏斗统计（`api.rs:462-491`）；停滞商机（`api.rs:1169-1213`） | **无阶段状态机**：`stage` 自由字符串，无 CHECK（`0005_create_opportunity.sql:13`），创建/更新直接落库（`opportunity.rs:293`、`338`）；「阶段推进不可逆」（`docs/07:247`）未实现；**赢单/输单只是 stage 的某个字符串**，无 `won_at/lost_at/赢单原因/输单原因/竞争对手/概率/预计成交额` 字段（`0005:7-23`）；`customer_id` 不校验归属（`opportunity.rs:290`）；无「商机→合同」转化接口，只在前端拼一次 `POST /contracts`（`OpportunityDetailView.vue:11`） |
| 4 | 商机阶段历史 | **80%** | 触发器自动记录 INSERT 与 stage 变更，只增不改（`0016_opportunity_stage_history.sql:30-50`）；回填历史（`0016:22-28`）；查询接口（`api.rs:1001-1037`）；停留时长（`api.rs:1046-1080`）；阶段转化率（`api.rs:1103-1132`） | `changed_by` 取 `NEW.updated_by`，但 API 更新时才写 `updated_by`，导入路径 `updated_by` 为 NULL（`import.rs:504-522` 未写 `updated_by`）；删除的商机历史仍参与统计（`api.rs:1057-1068` 无 `c.deleted_at IS NULL`）；无客户状态历史（`crm.customer.status` 变更不留痕，只有审计 JSON） |
| 5 | 合同 Contract | **55%** | 五接口（`contract.rs:102-106`）；`paid_cents` 预聚合列（`0011:12-22`）；`opportunity_id` 关联（`0015`）；状态字段 + 前端四态（`ContractsView.vue:155-160`） | **无状态机**：`status` 无 CHECK（`0006_create_contract.sql:14`），API 任意值（`contract.rs:305`、`352`）；`contract_no` **无唯一索引**（`0006:11`），可重复开票；`amount_cents int` 上限约 2147 万元（`0006:13`）；无审批流（`docs/06:293` 要求复用工作流）；`amount_cents` 可改小到小于 `paid_cents`，无校验（`contract.rs:351`） |
| 6 | 回款 Payment | **50%** | 五接口 + `paid_from/paid_to` 过滤（`payment.rs:79-89`、`109-216`）；触发器维护 `contract.paid_cents`（`0011/0012`）；按日汇总 `payment_daily`（`0014`）；覆盖索引（`0010`、`0013`） | **无退款/冲正实体**（无 `kind`/`reversed_by` 字段，`0007:7-23`）；`amount_cents` 无 `> 0` 校验（`0007:12`、`payment.rs:296-308`），可录负数「假退款」；`contract_id` 无外键（`0007:10`）；**软删不冲减预聚合**（见 P0-2）；无超额校验（`payment.rs:283-308`），`remaining_cents` 可为负（`api.rs:598`） |
| 7 | 退款 / 冲正（CRM） | **0%** | — | 表、接口、权限点、前端全无；`grep refund` 在 `plugins/crm` 零命中；CONTEXT/docs 未定义 CRM 退款，但「回款」业务闭环必然需要（红冲）——目前只能靠负数回款，无痕迹、无审批、报表无法区分 |
| 8 | 公海（Pool） | **0%** | — | `docs/06:294`「公海池用归属+回收规则」、`docs/07:246`「30 天未跟进自动回收」均未实现：`crm.customer` 无 `pool`/`claimed_at`/`last_follow_at` 字段（`0001:10-31`），无领取/回收接口，无 `iam.user.disabled` 订阅（`CONTEXT.md:63`） |
| 9 | 跟进记录（时间线） | **0%** | — | `docs/06:294`「跟进记录用时间线」未实现：无 `crm.follow_up`/`activity` 表，无接口，`CustomerDetailView.vue` 只有商机/合同/联系人三个 tab；「30 天未跟进」也因此无法计算 |
| 10 | 经营看板 Dashboard | **70%** | 汇总/趋势/TOP 客户（`api.rs:645-807`、`918-956`）；前端 `DashboardView.vue` 含阶段停留、转化、停滞三张分析表；权限复用 `customer:list`（`api.rs:650`） | 月度回款两条路径口径不同（见 P1-7）；`paid` 来自 `contract.paid_cents`，受软删漂移与 void 过滤双重影响（`api.rs:669-683`）；「回款额」把作废合同的已收款整体剔除（`api.rs:673`），真实现金流消失；无同比/环比、无销售个人业绩 |
| 11 | Pipeline 视图 | **45%** | 只读漏斗：阶段聚合+金额+赢单率（`PipelineView.vue:1-120`、`api.rs:462-491`）；菜单已挂（`lib.rs:193-200`） | **不是 Pipeline（看板）**：无按阶段分列卡片、无拖拽推进、无阶段变更 API（`opportunity.rs:88-95` 只有 list/detail/create/update/delete）；未知 stage 的商机被前端直接丢弃，`totalCount`/`winRate` 失真（`PipelineView.vue:77-93`） |
| 12 | Excel 导入 / 导出 | **50%** | 客户导入导出（`excel.rs`）；合同/回款/联系人/商机导入（`import.rs`）；合同/回款/联系人/商机导出（`export.rs`）；异步导出任务框架（`exports.rs`）；前端 `useImport/useExport` 有成功/失败提示 | 客户导出表头错位（P0-1）；导入无 SAVEPOINT（P0-3）；导入静默截断 2000 行（P1-11）；金额 `parse::<f64>()`（P1-12）；导入不校验 `level`/`email`/合同金额与回款额一致性 |
| 13 | 权限点 | **80%** | 23 个权限点声明并自动 upsert（`lib.rs:61-156`、`app.rs:70`）；所有 handler 都 `perm::require`（`api.rs:38` 等 30 余处）；前端 `v-perm` 全按钮覆盖（`CustomersView.vue:13-56` 等） | 4 个实体缺 `:get`（详情复用 `:list`：`contact.rs:217`、`opportunity.rs:223`、`contract.rs:234`、`payment.rs:232`）；4 个实体的导出/导入复用 `:list`/`:add`（`export.rs:126,133,257,265`、`import.rs:156,303,403,546`），导致「有列表权=可全量导出」，与 `crm:customer:export/import` 的粒度不一致（`docs/07:255` 只定义了 customer 的 export） |
| 14 | 审计 | **75%** | 15 处写接口全有 `record_user`（`api.rs` 3、`contact.rs` 3、`contract.rs` 3、`opportunity.rs` 3、`payment.rs` 3、`excel.rs` 1）；update 用整行 before/after（`audit::snapshot`，`audit.rs`）；导入记录汇总（`excel.rs:200-210`） | 审计在**业务事务外**（P1-6）；状态变更无独立业务动作（`crm.customer.status_change`），只能靠 `crm.customer.update` 里翻 JSON；`audit.rs:14-16` 自述「M1 简化版，强一致应走 outbox」 |
| 15 | 领域事件 | **0%** | — | 全仓搜索 `outbox` / `EventBus` / `publish(` 仅命中注释（`audit.rs:15`）；`CONTEXT.md:61-63` 的 3 个事件（`customer.created` / `customer.transferred` / 订阅 `iam.user.disabled`）一个都没有；插件间协作（合同审批→消息提醒）也因此无法落地 |
| 16 | CONTEXT.md 自身 | **25%** | 客户域写得很准（§1-§7 与代码逐条对得上） | 只覆盖 `customer`：表清单仅 1 张（`CONTEXT.md:20`），实际 7 张；权限点仅 5 个（`:26-30`），实际 23 个；接口仅 5 个（`:38-42`），实际 39 个端；`lib.rs:6` 还写着「后续会扩展联系人、商机、合同、回款」——文档落后实现约 3 个迭代 |

### 1.2 租户与计费域（0055–0066 + `tenant_usage.rs` + `system.rs`）

| 子域 | 完成度 | 已实现（证据） | 缺口（证据） |
|---|---|---|---|
| 配额（用户/存储） | **70%** | 字段与 0=不限语义（`0055:5-8`、`0056:5-8`）；建用户校验+80% 预警（`system.rs:632-648,724-735`）；上传校验（`files.rs:157-175`）；导入前预检（`user_import.rs:280-295`）；改配额时校验现值（`system.rs:5762-5770`） | 全部是「读-判断-写」，无行锁/无约束 → 并发可超额（P1-8 同类）；存储配额先 `storage.save` 再 `INSERT sys_file`（`files.rs:181-199`），插入失败留孤儿文件且不占配额；停用/删除租户不释放配额语义 |
| 账单状态机 | **65%** | 4 态 + CHECK（`0060:15-19`）；显式迁移表 `draft→issued/void`、`issued→paid/void`（`system.rs:6644-6653`）；状态变更审计（`6660-6668`） | 「paid」可被人工置上而无任何流水（`update_bill_status` 不校验 `paid_cents`，`system.rs:6628-6670`）；`paid→void` 不允许、`void` 不可恢复；**生成账单会覆盖 issued/paid**（P0-4） |
| 逾期分级 | **70%** | `overdue_tier` 字段（`0064`）；30/60/90 分档 + 7 天抑制 + 升级不等抑制（`system.rs:7420-7452`）；每日扫描（`app.rs:227-236`） | **无到期日**：逾期天数=「账期结束+宽限」起算（`system.rs:7409`），不是账单 due_date；`overdue_tier` 永不重置（全仓仅 `7445` 一处写入），退款回到 issued 后档位仍是旧值；扫描无 advisory lock，手动+定时并发会重复告警（P2-8） |
| 催收 | **65%** | 催收表（`0065`）+ 列表/新增（`system.rs:7515-7595`）；承诺还款提醒扫描（`7606-7633`）；`reminded_at` 7 天抑制（`0066`） | 只有 note/承诺金额/日期，无催收方式、无下次跟进、无结果；`create_bill_collection` 允许对 `paid` 账单催收（`7560`）；提醒扫描不 join 账单，已付清账单的承诺仍会提醒 |
| 退款 / 冲正 | **55%** | `kind` 字段 + CHECK（`0062:5-13`）；净额算法统一用 `CASE WHEN kind='refund' THEN -amount`（`system.rs:6596,6769,6864,6960`）；退款后 paid→issued（`6886-6893`）；审计（`6895-6906`） | 无行锁/幂等键 → 并发重复退款（P1-8）；`bill_id` 不校验租户（P1-9）；`docs/05:1302` 写「插入负向流水」但代码插正值+kind（`6876`），文档与实现相反；退款不回滚 `overdue_*` |
| 月末自动生成 | **30%** | 启动即跑 + 24h 循环（`app.rs:208-222`）；`generate_bills` 幂等 upsert（`tenant_usage.rs:220-245`） | **循环用 `if now.format("%d")=="01"` 判断，只在「进程恰好在 1 日跑到」时生成**：启动即 tick 一次，之后固定 `sleep(24h)`，长驻进程下一次 tick 一定在 2 日 → 全月不生成；也没有「补跑漏掉月份」的逻辑（P1-10）；UTC 与 PG `now()/current_date` 时区混用 |
| 对账 | **60%** | 按租户汇总应收/收款/退款/净收/未收（`system.rs:6952-6983`）；Excel 三 sheet（`7114-7383`） | 应收包含 `void` 账单（`6954-6957` 无 status 过滤）；CSV 导出（`6405-6494`）**不读已生成账单，而是现场用当前单价重算**，与 issued 账单可能不一致；Excel「金额(分)」列套 `#,##0.00` 货币格式（`7182`），12345 分显示成 `12,345.00`，易被读成元 |
| 用量快照 | **80%** | 日快照 + 唯一键 + upsert（`0058:5-16`、`tenant_usage.rs:87-100`）；用户/文件/存储/客户/合同五指标；尖峰告警（`112-131`）；用量历史查询（`139-159`） | 内核直查 `crm.*`（P2-12）；`snaps==0` 时用「当前值」代替历史月均值（`198-215`），补生成历史月账单会按今天的数据计费；每租户一个事务，中途失败已提交部分不回滚 |

### 1.3 生成器 `crates/xtask/src/gen.rs`

| 维度 | 结论 | 证据 |
|---|---|---|
| 实际产物 | **只有 2 个文件**：`migrations/{seq}_create_{table}.sql` 与 `src/{table}.rs` | `gen.rs:87-99`（`fs::write(sql_path…)`、`fs::write(rs_path…)`）；`gen.rs:101-116` 其余全靠打印提示人工操作 |
| 后端四层 | 否，只有单文件（VO+3 个 Request+Query+5 handler+router），无 repo/service 分层 | `gen.rs:405-683` 模板 |
| RLS / TenantTx | TenantTx ✅；**RLS 完全没有**（不生成 ENABLE/FORCE，也不生成 schema 限定） | 模板 `gen.rs:200-228` 无 RLS；自己承认「生成器只建表，策略需要显式补」`0005_create_opportunity.sql:31` |
| 权限点 | **不生成**：模板无 `perm::require`，也不写 `permissions()` | `gen.rs:405-683` 全文无 `require`/`permission` 字样 |
| 审计 | **不生成**：无 `audit::record_user`、无 before/after | 同上（`Select-String audit` 在 gen.rs 零命中） |
| OpenAPI | 单函数有 `#[utoipa::path]`，但**不注册**到插件的 `paths(...)`，靠打印提示人工加 | `gen.rs:113-115` |
| 前端页面 | **不生成** | 全文件无 `vue`/`frontend` 字样 |
| 测试 | **不生成**（仅 gen.rs 自己的 3 个单测） | `gen.rs:701-778` |
| id / 审计字段 | ✅ 有 `id/tenant_id/version/created_at/created_by/updated_at/updated_by/deleted_at`，`created_by` 自动绑当前用户 | `gen.rs:207-216`、`gen.rs:331-333` |
| 对已存在表重跑 | **直接覆盖源码**：`fs::write` 无 `exists()` 检查、无 `--force`、无备份、无 diff；迁移则新起一个序号 + `CREATE TABLE IF NOT EXISTS`（静默 no-op，留下 schema 漂移） | `gen.rs:90-99`、`gen.rs:686-699`；`docs/07:461` 已把「重跑会覆盖」写成注意事项 |
| 与文档承诺的差距 | `AGENTS.md:66`、`docs/企业级管理后台-架构设计.md:41` 承诺「后端四层 + 前端页面 + 迁移 + 权限 + 测试」，实际约 **20%** | 同上 |

### 1.4 后台任务与异步导出

| 维度 | 完成度 | 结论与证据 |
|---|---|---|
| 任务状态机 | **70%** | `pending→running→success/failed`，CHECK 约束（`0018_export_task.sql:14,22`）；`recover_stale` 启动收尾（`exports.rs:501-510`）。缺 `cancelled`、无进度百分比、无 `attempt` 计数（`0018:8-23`） |
| 失败重试 | **0%** | 无重试、无退避、无死信；失败即终态（`exports.rs:290-293`、`486-498`）；`recover_stale` 用一条 UPDATE 把**所有租户**的 `pending/running` 置 failed（`501-505`），多实例滚动发布时会把别实例正在跑的任务误杀 |
| 文件清理 | **20%** | 只有「删除任务时顺手删文件」（`exports.rs:386-390`）；**无 TTL、无定时清理任务**（`jobs.rs:27-67` 只清会话与审计告警）；任务行删除后文件才消失，用户不删则永久占用对象存储 |
| 20k 行上限 | **50%** | 异步导出硬编码 `LIMIT 20000`（`exports.rs:551,614`；`export.rs:22,83,152,206`）；**无截断标记、无「已截断」提示**，`total_rows` 只是实际导出数，用户无法知道还有数据；同步导出 5000（`excel.rs:28,64`）同样静默截断；导入方向反向静默截断 2000（`import.rs:44,184,329,431`） |
| 并发/资源 | **40%** | 每次 `create_task` 无脑 `tokio::spawn`（`exports.rs:275`），无并发上限、无队列；任务全程占一条 DB 连接 + 全量 xlsx 在内存（`444-465`）；文件先落盘再 commit（`465-481`），commit 失败留孤儿文件 |
| 下载鉴权 | **90%** | 按 `tenant_id + user_id` 取任务（`exports.rs:313-321`），下载前校验 `success`（`325-330`） |

---

## 二、数据正确性缺陷

### P0（会直接产生错误账/错数，或让用户以为成功其实没落库）

**P0-1　客户 Excel 导出表头与列错位，导出文件无法回灌**
- 证据：表头 5 列 `["客户编码","客户名称","客户等级","联系电话","邮箱"]`（`plugins/crm/src/excel.rs:25`）；数据行 6 列，写入顺序 `code, name, level, status, phone, email`（`excel.rs:88-99`，类型注释也自证 6 元组 `excel.rs:30-38`）。
- 错位结果：第 4 列「联系电话」实际写 `status`（`potential`/`signed` 等英文枚举）；第 5 列「邮箱」实际写 `phone`；`email` 落到**无表头的第 6 列**。
- 影响：① 导出的客户表在业务侧完全是错的（电话列显示 `potential`）；② 把导出文件改完再导入时，`联系电话`→写进 `phone` 的其实是状态值，`邮箱`→写进 `email` 的其实是手机号，真实邮箱被静默丢弃；③ 由于导入按表头名匹配（`excel.rs:238-259`），这个错位不会被任何校验拦住。
- 修复：`HEADERS` 补 `"客户状态"` 放在第 4 位（或导出时去掉 status），使 6 个表头 ↔ 6 列严格一一对应；补一条「导出→解析→行相等」的回归测试。

**P0-2　回款软删不冲减预聚合：`contract.paid_cents` 与 `payment_daily` 永久漂移**
- 证据：`sync_contract_paid` 的 UPDATE 分支是「先减 OLD 再加 NEW」（`0011_contract_paid_cents.sql:39-50`，修正版 `0012_fix_trigger_search_path.sql:26-36`）。软删是一条 `UPDATE crm.payment SET deleted_at = now()`（`payment.rs:425-428`），触发器看到 `OLD.amount = NEW.amount`，**净变化为 0** → `paid_cents` 不变。`payment_daily` 完全相同：`-OLD + NEW`（`0014_payment_daily.sql:43-59`）。
- 而两个预聚合的**回填**是带 `deleted_at IS NULL` 的（`0011:16-22`、`0014:22-29`），所以「初始正确、删一笔就永久错」。
- 受影响读数：合同详情「已回款/未回款」（`api.rs:543-557,595-603`）、经营看板回款额（`api.rs:669-683`）、TOP 客户已回款（`api.rs:931`）、合同导出「已回款(元)」（`export.rs:16,53`）、月度趋势快路径（`api.rs:733-746`）。
- 触发条件极常见：录错一笔回款 → 删除重录，账上就多出一笔永久虚增。
- 修复（任一）：① 触发器加 `IF NEW.deleted_at IS NULL AND OLD.deleted_at IS NULL` 等四种状态分支，把「软删/恢复」当成真实的 `-amount/+amount`；② 或回款只做硬删 + 归档表；③ 并补一条对账任务：`paid_cents` 与 `SUM(payment WHERE deleted_at IS NULL)` 定期比对告警。

**P0-3　Excel / 批量导入没有 SAVEPOINT，"部分成功"语义不成立**
- 证据：客户导入整文件一个事务，逐行 `insert_row`，最后才 `tx.commit()`（`excel.rs:186-198`）；插件导入框架同样整文件一个事务（`imports.rs:109-112`）；合同/回款/联系人/商机导入（`import.rs:44-143,184-290,329-390,431-533`）与用户导入（`user_import.rs:306-331`）都是同一模式；全仓 `grep SAVEPOINT` 零命中。
- PostgreSQL 语义：事务内任一语句报错（包括被代码捕获的 23505 唯一冲突）都会把事务置为 aborted，后续语句一律 `25P02`；最终 `COMMIT` 要么报错、要么静默回滚。
- 后果：`ImportResult{success, failed}`（`excel.rs:112-121`、`imports.rs:32-39`）在出现**第一条重码/超长/约束冲突**后即失真——前端会提示「导入完成：成功 N 行，失败 M 行」（`useImport.ts:35`），实际什么都没落库（或只落了一部分且不可预期）。
- 另：`ImportResult.errors` 只保留 50 条（`import.rs:61` 等），用户看不到全貌。
- 修复：每行 `SAVEPOINT row_n; ...; RELEASE`（出错 `ROLLBACK TO row_n`），或改为「预校验全量 → 全成功才写」，并把 `success` 改为以 DB 实际提交结果为准；文档要把「部分成功」写清楚到底是哪一种。

**P0-4　账单重算会覆盖已出账/已收款账单（`generate_bills` 缺 status 守卫）**
- 证据：`tenant_usage.rs:220-234` 的 `INSERT … ON CONFLICT (tenant_id, bill_month) DO UPDATE SET currency/avg_users/avg_storage_bytes/user_amount_cents/storage_amount_cents/total_cents/updated_at`，**没有 `WHERE sys_tenant_bill.status = 'draft'`**；函数注释却写「幂等，重复生成覆盖草稿」（`tenant_usage.rs:168`、`system.rs:6548`）。
- 可达路径：手动 `POST /tenants/bills/generate`（`system.rs:6556-6570`）、补生成 `POST /tenants/bills/backfill`（`7017-7048`，可一次跑 60 个月）、不可靠的月末自动任务（`app.rs:208-222`）。
- 后果：已 `issued`/`paid` 的账单金额被按「今天的单价/用量」改写，而 `sys_tenant_bill_payment` 的收款流水不变 → 对账立刻出现「已收 > 应收」或「未收 ≠ 应收-已收」，且被改写的历史账单无法从审计恢复（只记了 status 变更，`system.rs:6660-6668`，未记金额）。
- 修复：`ON CONFLICT … DO UPDATE … WHERE sys_tenant_bill.status = 'draft'`；非 draft 时改为「生成差异草稿/新版本」；生成动作必须落审计（月份、租户数、金额合计）。

### P1（口径错误、可越权、可重复写入，或边界会丢数据）

**P1-1　CRM 四张业务表没有外键，孤立/跨租户引用可写入**
- 证据：`contact.customer_id`（`0002:10`）、`opportunity.customer_id`（`0005:9`）、`contract.customer_id`（`0006:10`）、`payment.contract_id`（`0007:10`）均只有 `uuid NOT NULL`，无 `REFERENCES`；全仓 CRM 迁移只有 3 处外键（`0014:14`、`0015:10`、`0016:10-11`）。
- 后果：`payment.create` 可写一个不存在的 `contract_id`（`payment.rs:290-308` 不校验），触发器 `UPDATE crm.contract WHERE id=NEW.contract_id` 影响 0 行 → 回款记录存在但 `paid_cents` 不增；客户删除（软删）后其合同/回款仍可继续新增。
- 修复：补 FK（`ON DELETE RESTRICT`）+ 写入前校验客户/合同可见性。

**P1-2　`contract_no` / `payment_no` 无唯一索引**
- 证据：`0006:11`、`0007:11` 只有 `varchar(64) NOT NULL`；CRM 迁移里唯一的唯一索引是 `uq_customer_code`（`0001:48`）。导入用 `SELECT EXISTS(...)` 预检（`import.rs:99-112`、`246-259`），并发下仍可重复。
- 后果：同一份合同可重复开票、同一笔款可重复登记（叠加 P1-8 的并发问题），对账无法定位。

**P1-3　商机阶段无 DB 约束、无状态机**
- 证据：`stage varchar(32) NOT NULL` 无 CHECK（`0005:13`）；`create/update` 直接把请求值绑定（`opportunity.rs:293,338`），无白名单；导入侧才校验 `initial/proposal/negotiation/won/lost`（`import.rs:453`），前端也是这 5 个（`OpportunitiesView.vue:130-134`）。
- 后果：直连 API 可写任意 stage → 漏斗/停留/停滞统计（`api.rs:470-476,1196`）静默漏算；`stuck_opportunities` 用 `NOT IN ('won','lost')` 判「未结」（`api.rs:1196`），拼错一个 `Lost` 就永远算停滞。

**P1-4　合同 `status` 无约束、无状态机**
- 证据：`status varchar(32) NOT NULL` 无 CHECK（`0006:14`）；API 直写（`contract.rs:305,352`）；只有导入校验 4 值（`import.rs:76`）。
- 后果：看板用 `status <> 'void'` 排除作废（`api.rs:656,673,717,937`）；写进 `Void`/`voided`/`作废` 都不会被排除，作废合同继续计入营收。

**P1-5　客户状态机未实现，违反 CONTEXT.md 明文规则**
- 证据：`api.rs:238-242` 只判断「在 4 个值里」，`signed → potential` 直接放行；`CONTEXT.md:70-76` 要求「signed 不允许直接改为 potential（需先 lost 再复活）」「状态变更必须记录审计日志」（后半句 update 的通用审计可满足，前半句无实现）。

**P1-6　审计与业务不同事务，且先 commit 后写审计**
- 证据：全部 CRM 写接口都是 `kernel::audit::record_user(&state.db, …)`（`api.rs:201-210,283-291,336-344`、`contact.rs:295-303,381-389,452-460`、`contract.rs:314-322,404-412,475-483`、`opportunity.rs:301-309,387-395,458-466`、`payment.rs:310-318,396-404,467-475`、`excel.rs:200-210`），签名收 `&PgPool`（`audit.rs` `record_user`），**自开连接/事务**；且 `record_user` 在 `tx.commit()` 之后调用。
- 后果：① 业务回滚了但审计已写（审计"记录了没发生的事"）；② 审计写失败只打日志（`audit.rs:14`），关键变更无痕；③ 每个写请求同时占用 2 条连接（`TenantTx` + 审计），高并发下易打满连接池。
- 修复：`record_*` 增加 `&mut PgConnection` 变体，在业务事务内写。

**P1-7　看板月度回款两条路径口径不一致（同一租户两个数）**
- 证据：`data_scope == "all"` 走 `payment_daily`，按 `d.day`（= `COALESCE(paid_at, created_at)::date`，`0014:23,44,53`）聚合（`api.rs:733-746`）；受限用户走 `crm.payment`，按 `c.created_at`（录入时间）聚合（`api.rs:748-767`）。
- 后果：`paid_at` 与 `created_at` 跨月（补录上个月回款、导入历史回款，`import.rs:209-218` 就把 `paid_at` 设为历史日期）时，**财务（all）看到 3 月回款，销售经理（dept/self）看到 4 月回款**；且慢路径统计的是「录入月的金额」，不是「回款月的金额」。
- 修复：两条路径统一按 `COALESCE(paid_at, created_at)` 归月；或干脆给 `payment_daily` 增加 `owner_id/dept_id` 维度以支持受限用户。

**P1-8　账单收款/退款无行锁、无幂等键 → 并发重复收款/重复退款**
- 证据：`create_bill_payment` 先 `SELECT status,total_cents`（`system.rs:6756-6761`）再 `SELECT SUM(...)`（`6768-6774`）再 INSERT（`6778-6792`），全程无 `FOR UPDATE`、无唯一约束（`0061` 只有普通索引）；退款同理（`6851-6893`）。
- 后果：两个并发请求都读到 `paid=0`，都通过 `paid+amount <= total` 校验，双双插入 → 超收；或两笔并发退款都通过 `amount <= paid`，双倍退。
- 修复：`SELECT … FOR UPDATE` 锁账单行（或 `UPDATE … WHERE status='issued' AND paid+$1<=total` 的原子条件更新），并给收款流水加 `(bill_id, reference)` 幂等唯一键。

**P1-9　账单接口不校验 `id` 所属租户 + `sys_tenant_bill*` 完全没有 RLS**
- 证据：0058–0066 新建的四张表（`sys_tenant_usage_snapshot` / `sys_tenant_bill` / `sys_tenant_bill_payment` / `sys_tenant_bill_collection`）**没有一条 `ENABLE ROW LEVEL SECURITY`**（`Select-String "ROW LEVEL SECURITY"` 在 0055-0066 零命中）；`sys_tenant` 本身也无 RLS（`0012_rls_system.sql` 未覆盖）。
- 代码侧：`create_bill_payment`/`create_bill_refund` 按 `id` 取账单后**用账单的 `tenant_id` 落流水**（`system.rs:6756-6761,6785`、`6851-6857,6880`）；`list_bill_payments`/`list_bill_collections` 只按 `bill_id` 查（`6723-6728`、`7522-7527`）。
- 后果：任何拥有 `system:tenant:edit/list` 的账号（权限码是全局的，`perms.rs:90-100`）可对任意租户账单收款/退款/查流水；UUIDv7 时间有序，可被枚举放大。
- 修复：所有账单子查询带 `tenant_id = current_setting('app.tenant_id')`，并对 0058-0066 补 RLS + FORCE（或明确走「平台运营库」的独立连接与独立权限域）。

**P1-10　「月末自动生成账单」实际不生效**
- 证据：`app.rs:208-222` —— `loop { if now.format("%d") == "01" { generate_bills(上月) } sleep(24h) }`。第一次 tick 在启动瞬间，之后每隔 24h。**只有当进程恰好在每月 1 日启动（或上一轮 tick 恰好落在 1 日）时才会生成**；连续运行的服务下一次 tick 必然落在 2 日 → 整月不生成，且没有「发现缺月则补跑」的逻辑。
- 叠加：`Utc::now()` 与 PG `current_date/now()`（服务器本地时区）混用，跨时区会生成到错误月份。
- 后果：账单默认全部靠人工 `/bills/generate`，一旦漏跑，计费与对账整月缺失。
- 修复：改为「记录 last_generated_month，每分钟检查一次：若 `current_month > last_generated_month` 则补跑所有缺失月份」，并用同一个时区源（建议 DB 的 `date_trunc('month', now())`）。

**P1-11　导入/导出静默截断，用户无法感知**
- 证据：导入 `range.rows().enumerate().skip(1).take(2000)`（`import.rs:44,184,329,431`）——第 2001 行起**直接丢弃**，不报错、不计入 `failed`；导出 `LIMIT 20000`（`export.rs:22,83,152,206`、`exports.rs:551,614`）、同步导出 5000（`excel.rs:28,64`），无任何「已截断」标志。
- 后果：财务拿到的对账文件「少了 3 万行」而界面显示成功；导入「导了 5000 行只进去 2000 行」。
- 修复：超限直接 400 并提示分批；导出在 `ExportFile` 增加 `truncated: bool` + `total_available`，前端显式提示。

**P1-12　金额用 f64 参与计算（文档明令禁止）**
- 证据：导入金额 `amount_text.parse::<f64>()` 后 `(v*100.0).round() as i32`（`import.rs:66-75,127`、`199-208,273`、`460-472`）；计费 `avg_users/avg_storage: f64`、`(avg_users * f64::from(price)).ceil() as i64`（`tenant_usage.rs:185-219`、`system.rs:6383-6385,6463-6465`）；`docs/07:266` 明确「金额字段一律 Decimal，禁止 f64」。
- 额外风险：`as i32` 在溢出时**静默饱和**为 2147483647（Rust `float as int` 语义），一条 3000 万元的合同会变成 2147.48 万元而不报错。
- 修复：全部改 `rust_decimal` / `i64` 分运算；`amount_cents` 至少 `bigint`。

**P1-13　`contract.amount_cents` 是 `int`，上限约 2147 万元**
- 证据：`0006:13` `amount_cents int NOT NULL`，而 `paid_cents` 是 `bigint`（`0011:13`）。企业合同超 2147 万元即无法录入（sqlx 绑定 i32，超出直接序列化失败/溢出）。

**P1-14　计费、用量、账单四张新表无 RLS，租户隔离只靠应用层**
- 证据：`0058_tenant_usage_snapshot.sql:5-16`、`0060_tenant_bill.sql:5-20`、`0061_tenant_bill_payment.sql:5-15`、`0065_tenant_bill_collection.sql:5-14` 均无策略；对比 `0012/0015` 的内核表与 CRM 的 `0003/0004`，属于回退。
- 后果：任何一处漏写 `tenant_id` 就是全租户数据泄露（P1-9 已给出一个真实入口）。

### P2（可维护性、口径瑕疵、边界与文档一致性）

- **P2-1　数据权限片段 20 份内联拷贝，与「唯一实现」自相矛盾**：`contact.rs/opportunity.rs/contract.rs/payment.rs` 各 5 处（`LIKE '%all%'` 共 20 处），内核 `scope.rs:19-36` 一份，生成器模板 `gen.rs:356-378` 一份。`scope.rs:3-5` 声称「唯一实现，避免漂移」，实际有 3 个来源。当前文本一致，但任何规则调整都要改 6 处。
- **P2-2　`LIKE '%all%'` 用子串匹配枚举**：`scope.rs:19-36`。当前 5 个取值（`0023:11`）恰好不互相包含，但未来出现 `all_dept`、`self_and_dept` 之类就会**静默放大权限**。应改为 `scope = 'all'` 等精确比较。
- **P2-3　生成器缺 RLS/权限/审计/OpenAPI/前端/测试，且重跑覆盖源码**（详见 1.3）。这是「新表默认裸奔」的根因（`0005:31` 注释即证据）。
- **P2-4　无超额回款校验，`remaining_cents` 可为负**：`payment.rs:283-308` 不校验合同剩余额度；`api.rs:598` 直接 `amount - paid`；前端进度条 `Math.min(100, …)`（`ContractDetailView.vue:120`）把负数掩盖了。
- **P2-5　联系人可挂任意客户 / 多个主联系人**：`contact.rs:268-307`（create）、`318-393`（update，还允许改 `customer_id`），无 `ensure_customer_visible`（对比 `api.rs:351-377` 的同类函数）；`0002:14` 无部分唯一索引 `(customer_id) WHERE is_primary AND deleted_at IS NULL`。
- **P2-6　客户编码长度按字节判断**：`api.rs:166`、`excel.rs:284` 用 `req.code.len() > 32`，而列是 `varchar(32)`（字符）；11 个汉字（33 字节）会被应用层拒绝，与 DB 语义不一致（名称用的是 `chars().count()`，`api.rs:171`）。
- **P2-7　OpenAPI 契约缺 `/api/crm/dashboard/top-customers`**：路由已挂 `lib.rs:253`，但 `CrmApi` 的 `paths(...)` 未列 `top_customers`（`lib.rs:272-284`），`contracts/openapi.json` 全文搜不到该 path → `pnpm gen:api` 生成的 TS 类型缺失，前端只能手写接口。
- **P2-8　逾期/催收扫描的并发与状态复位**：`scan_overdue_bills_internal` 无 advisory lock（`system.rs:7420-7452`），手动接口（`7464-7475`）与 24h 定时（`app.rs:227-236`）可同时命中同一账单重复告警；`overdue_tier` 只在 `7445` 写入、从不因付清而复位。
- **P2-9　对账把作废账单计入应收**：`system.rs:6954-6957`（`bills` CTE 无 `status <> 'void'`）、`7150-7172`（Excel 同 SQL）。作废账单仍占「应收/未收」。
- **P2-10　异步导出无文件 TTL / 无重试 / 无并发上限**：`exports.rs:275`（裸 spawn）、`486-510`（无重试）、`jobs.rs:27-67`（无清理任务）、`465-481`（先存文件后 commit，失败留孤儿）。
- **P2-11　「金额(分)」列套货币格式**：`system.rs:7182` `set_num_format("#,##0.00")` 用于分单位数值（`7232-7240`、`7282`、`7341-7354`），12345 分显示为 `12,345.00`，财务极易误读为 12345 元。
- **P2-12　内核直查插件表，破坏插件边界**：`system.rs:6090-6124`（租户详情查 `crm.customer/opportunity/contact/contract/payment`）、`tenant_usage.rs:70-86`（快照查 `crm.customer/contract`）。`arch_check` 只 grep `kernel/Cargo.toml` 的 `plugin-*` 依赖（`crates/xtask/src/main.rs:195-205`），抓不到 SQL 层耦合；`to_regclass` 只探测 `crm.customer`（`6090`），其余表被删会直接 500。
- **P2-13　`recover_stale` 全局置失败**：`exports.rs:501-505` 一条 UPDATE 跨所有租户把 `pending/running` 置 failed，无 `started_at` 时间过滤 → 滚动发布时误杀其他实例任务。
- **P2-14　导入/导出无文件体积上限**：`excel.rs:152-168`、`imports.rs:85-101` 把整个 multipart 读进内存（`.bytes().to_vec()`），默认 axum body 限制之外无业务上限，大文件可 OOM。
- **P2-15　`customer.dept_id` 可空 + 建客户时直接取 `user.dept_id`**：`0001:19`、`api.rs:192`。管理员的 `dept_id` 为空时，该客户对 `dept`/`dept_and_child` 范围的用户**永久不可见**（`scope.rs:21,29` 对 NULL 不匹配）。
- **P2-16　回款排序口径不一致**：列表按 `created_at DESC`（`payment.rs:152`），索引 `idx_payment_contract` 与导出按 `paid_at DESC NULLS LAST, created_at DESC`（`0013:14-17`、`export.rs:82`）；同一实体两套时间语义，跨月补录时列表顺序与索引/对账顺序不同。
- **P2-17　`docs/05:1302` 与实现相反**：文档说退款「插入 kind=refund 的负向流水」，代码插入**正值** `amount_cents` + `kind='refund'`（`system.rs:6876`），且 `0061:9` 有 `CHECK (amount_cents > 0)`。按文档写报表会双倍计退款。
- **P2-18　租户过滤不统一：多处 JOIN / 存在性校验不带 `tenant_id`，隔离 100% 依赖 RLS**。`ensure_customer_visible` 的 WHERE 只有 `id + deleted_at + scope`（`api.rs:356-364`）；`stuck_opportunities` 的 `JOIN crm.opportunity c / crm.customer cu` 只按 id 关联（`api.rs:1193-1194`）；`stage_duration` 的 `JOIN crm.opportunity`（`api.rs:1065`）、`export.rs` 的 `JOIN crm.contract`（`79`）、`JOIN crm.customer`（`148`、`202`）均无 `tenant_id`。只要有人执行 `ops/disable_rls.sql`（该脚本真实存在且被文档 `docs/03:365` 推荐为回滚手段）或对某表 `NO FORCE`，这些 JOIN 立刻变成跨租户读取。列表/详情主查询是带 `tenant_id` 的，**唯独这些连接与存在性判断不带**，属于"看起来一致其实不一致"。
- **P2-19　软删口径不统一**：父对象软删后子对象行为未定义——`contract_payments` 对已软删合同直接 404（`api.rs:543-557`），但其下的 `crm.payment` 仍存在且 `paid_cents` 仍保留；看板把软删合同的回款整体剔除（`api.rs:671-683`），而合同导出也剔除（`export.rs:19`），`payment_daily` 却仍包含（P0-2）；`crm.customer` 软删不校验是否存在合同/回款（`api.rs:312-334`）。建议统一为「父软删 → 子不可写、历史可读、统计口径单列」。`posts.rs:296` 的软删还不 `version+1`，与 CRM 的软删约定也不一致。

---

## 三、已做对的部分

1. **软删 + 部分唯一索引**：`uq_customer_code (tenant_id, code) WHERE deleted_at IS NULL`（`0001:48-50`），并把 23505 精确映射为 409「客户编码已存在」（`api.rs:213-215`）。删除后编码可复用，语义正确。
2. **越权与不存在一律 404**，不泄漏资源存在性（`api.rs:141-142,279,332-334`；各实体 detail 同款）。
3. **RLS 覆盖到位（CRM 侧）**：customer/contact（`0003:8-18` + `0004:7-8`）、opportunity（`0005:32-38`）、contract（`0006:32-37`）、payment（`0007:31-36`）、payment_daily（`0014:31-35`）、stage_history（`0016:52-58`）全部 `ENABLE + FORCE`，且 handler 统一走 `TenantTx`（`tenant.rs:54-85`），`set_config(...,true)` 事务级不串租户。
4. **金额一律整数分**（`*_cents`），前端统一 `/100` 展示（`PipelineView.vue:104-106`）；没有把金额存成浮点列（浮点只出现在导入解析和计费算式里，见 P1-12）。
5. **触发器与业务同事务 + schema 限定名修复**：`0012_fix_trigger_search_path.sql:11-38` 把函数体全部改为 `crm.contract`，并有 0011→0012 的完整事故记录（`0012:4-8`）。这是很成熟的排障文档。
6. **索引设计有实测依据**：`tenant_id` 最左、部分索引对齐软删、`INCLUDE (amount_cents)` 覆盖索引、并把索引排序键改成与 `ORDER BY paid_at DESC NULLS LAST, created_at DESC` 完全一致（`0009`→`0010`→`0013` 的演进注释 + `docs/03:616-649` 的压测数据）。
7. **回款明细分页**：默认 50、上限 200（`api.rs:559-562`），并用 `count(*)` 给真分页（`564-574`），避免大合同把响应撑爆。
8. **阶段历史用触发器只增不改**（`0016:30-50`），并提供停留时长/阶段转化/停滞商机三个分析口径（`api.rs:1046-1213`），这是把「销售流程卡点」做进数据层的正确姿势。
9. **审计用整行 `to_jsonb` 快照**（`audit.rs` `snapshot`），update 前后各取一次（`api.rs:237,281`），比只记字段名强得多；15 个写接口无遗漏。
10. **前端按钮权限齐全**：`v-perm` 覆盖 CRM 全部操作按钮（`CustomersView.vue:13-56`、`ContractsView.vue:7-59`、`PaymentsView.vue:7-53`、`OpportunitiesView.vue:13-54`、`ContactsView.vue:13-49`），并注释了「后端仍独立校验」（`CustomersView.vue:126`）。
11. **计费 Excel 三段表头与列严格对齐**：账单 10/10（`system.rs:7201-7212` vs `7222-7241`）、收款流水 6/6（`7266` vs `7276-7287`）、对账 8/8（`7312-7321` vs `7332-7354`），还带合计公式与冻结窗格（`7244-7252,7289-7296,7356-7365`）——与 P0-1 形成鲜明对比。
12. **账单收退款业务规则落实**：仅 `issued` 可收款（`6765-6767`）、收款不超未收（`6775-6777`）、收满自动 `paid`（`6793-6800`）、退款不超净收（`6870-6872`）、退款后回退 `issued`（`6886-6893`）、`kind` CHECK（`0062:11-13`）。
13. **计费口径统一用同一段净额 SQL**：`CASE WHEN kind='refund' THEN -amount ELSE amount`（`6596,6769,6864,6960`），四处一致，没有各写一套。
14. **配额双轨 + 80% 预警**：用户数与存储分别校验（`system.rs:632-648`、`files.rs:157-175`），达到 80% 写告警（`724-735`），`0 = 不限制` 语义清晰（`0055:8`、`0056:8`）。
15. **用户导入有 `dry_run` 与文件内重名预检**（`user_import.rs:304-318`），错误报告可回导 xlsx（`173-232`）——这是 CRM 导入应该对齐的成熟做法。
16. **`ensure_default_menus` / `sync_plugin_permissions` 幂等**（`app.rs:70-79`），权限点声明与落库解耦，新租户自动补齐（`5648-5651`）。

---

## 四、建议修复顺序（按「错数风险 / 改动成本」排）

| 顺序 | 事项 | 预估 | 理由 |
|---|---|---|---|
| 1 | P0-1 客户导出表头 | 0.5d | 一行常量 + 一条回归测试，收益最大 |
| 2 | P0-2 软删冲减预聚合 | 1d | 触发器改 4 分支 + 一次性对账脚本修正历史漂移 |
| 3 | P0-3 导入 SAVEPOINT | 1d | 三处导入统一改造，抽公共 `run_in_savepoint` |
| 4 | P0-4 账单只覆盖草稿 | 0.5d | SQL 加 `WHERE status='draft'` + 审计 |
| 5 | P1-10 月末生成改「补跑缺失月」 | 0.5d | 否则 P0-4 修的是一台不跑的机器 |
| 6 | P1-1/2/3/4 约束与状态机 | 3d | 迁移补 FK/UNIQUE/CHECK，服务层补状态迁移表 |
| 7 | P1-6 审计入事务 | 2d | 涉及 15 处调用 + 内核函数签名 |
| 8 | P1-7 看板月度口径 | 0.5d | 两条 SQL 统一归月字段 |
| 9 | P1-8/9 账单并发与租户隔离 | 2d | `FOR UPDATE` + RLS 迁移 + 全量租户条件 |
| 10 | P1-11/12 截断与 f64 | 2d | 涉及导出框架与全部导入 |

> 说明：本文件仅做只读审计，未修改任何项目代码；`plugins/crm`、`crates/**`、`migrations/**` 均未触碰，唯一写入为本报告 `D:\fcj\zcadmin\.audit\data-domain.md`。
