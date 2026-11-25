# Workflow 插件 · 领域上下文（可插拔企业审批流）

> 给「人 + AI」看的业务内存。改代码前先读这里，不要靠猜。
>
> **可插拔说明**：本插件面向需要内部流程管控的企业（合同特批、折扣审批、用印/退款申请等）。
> 若某内部系统无需审批模块，可通过编译期 Cargo feature（`--no-default-features`）或运行期配置
> `DISABLED_PLUGINS=workflow`（或 `config/default.toml` 中 `[plugins] disabled = ["workflow"]`）一键拔除。

---

## 1. 领域术语

| 术语 | 含义 |
|---|---|
| 审批单（WorkflowRequest） | 一次业务审批申请，租户内 `code` 唯一；对应 `workflow.workflow_request` |
| 业务类型（biz_type） | 审批单关联的业务分类，如 `contract`（合同审批）、`discount`（折扣特批）、`refund`（退款审批）、`general`（通用申请） |
| 发起人（created_by） | 提交审批单的用户，数据权限 `self` 的依据 |
| 归属部门（dept_id） | 发起时的归属组织，数据权限 `dept` / `dept_and_child` / `custom` 的依据 |
| 状态机（status） | `pending`（待审批）→ `approved`（已通过） / `rejected`（已驳回） / `cancelled`（已撤回） |

## 2. 数据表

| 表 | 说明 | 关键约束 |
|---|---|---|
| `workflow.workflow_request` | 审批单主表 | 含 `tenant_id, created_at, created_by, deleted_at, version`；启用并 `FORCE ROW LEVEL SECURITY` |

## 3. 权限点

| 权限点 | 说明 |
|---|---|
| `workflow:request:list` | 审批单列表查询 |
| `workflow:request:get` | 审批单详情查看 |
| `workflow:request:add` | 发起审批申请 |
| `workflow:request:approve` | 审批处理（通过 / 驳回 / 撤回） |

> 权限点由 `WorkflowPlugin::permissions()` 自声明，启动时由内核幂等 upsert 到 `sys_permission`。

## 4. 接口

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/workflow/requests` | 分页列表（支持 `status` / `bizType` / `keyword` 过滤，带数据范围） |
| GET | `/api/workflow/requests/{id}` | 审批单详情（越权或不存在均返回 404） |
| POST | `/api/workflow/requests` | 发起审批单（初始状态为 `pending`） |
| POST | `/api/workflow/requests/{id}/decide` | 审批决议流转（`approve` / `reject` / `cancel`，带乐观锁版本递增与审计日志） |

## 5. 状态机规则

```text
pending（待审批）
  ├─ approve ──▶ approved（已通过，终态）
  ├─ reject  ──▶ rejected（已驳回，终态）
  └─ cancel  ──▶ cancelled（已撤回，终态）
```

- 只有 `pending` 状态的审批单允许执行决议流转；已处于终态的审批单再次决议返回 `422 Validation`（或并发冲突）。
- 每次决议更新均校验 `status = 'pending'` 并递增 `version = version + 1`，写入审计日志 `workflow.request.decide`。
