# CRM 插件 · 领域上下文

> 给「人 + AI」看的业务内存。改代码前先读这里，不要靠猜。

---

## 1. 领域术语

| 术语 | 含义 |
|---|---|
| 客户（Customer） | 签约主体，租户内 `code` 唯一；对应 `crm.customer` |
| 负责人（owner） | 客户的销售负责人，数据权限 `self` 的依据 |
| 归属部门（dept） | 客户归属组织，数据权限 `dept` / `dept_and_child` 的依据 |
| 生命周期 | potential → following → signed / lost |

## 2. 数据表

| 表 | 说明 | 关键约束 |
|---|---|---|
| `crm.customer` | 客户主数据 | `(tenant_id, code)` 部分唯一（未删除）；`status` 受 CHECK 约束 |

## 3. 权限点

| 权限点 | 说明 |
|---|---|
| `crm:customer:list` | 列表查询 |
| `crm:customer:get` | 详情 |
| `crm:customer:add` | 新增 |
| `crm:customer:edit` | 编辑 |
| `crm:customer:delete` | 删除（软删） |

> 权限点由 `CrmPlugin::permissions()` 声明，启动时自动 upsert。

## 4. 接口

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/crm/customers` | 分页列表（keyword/level/status 过滤） |
| POST | `/api/crm/customers` | 新建 |
| GET | `/api/crm/customers/{id}` | 详情 |
| PATCH | `/api/crm/customers/{id}` | 部分更新 |
| DELETE | `/api/crm/customers/{id}` | 软删 |

## 5. 数据权限

所有查询都带以下条件（与系统管理一致）：

```
all            不过滤
dept_and_child dept_id 属于当前部门子树
dept           dept_id = 当前用户部门
self           owner_id = 当前用户
```

越权访问一律返回 **404**（不泄漏资源是否存在）。

## 6. 事件

| 方向 | 事件 | 说明 |
|---|---|---|
| 发布 | `crm.customer.created` | M2 接入 outbox 后生效 |
| 发布 | `crm.customer.transferred` | 客户转移负责人时触发 |
| 订阅 | `iam.user.disabled` | 负责人离职 → 客户回公海（待实现） |

## 7. 状态机

```
potential（潜在）
   ↓ 跟进
following（跟进中）
   ├─→ signed（已签约）
   └─→ lost（已流失）  ← 可复活为 following
```

规则：
- `signed` 不允许直接改为 `potential`（需先 `lost` 再复活）。
- 状态变更必须记录审计日志。

## 8. 禁止事项

- 不得查询其他插件的 schema（`crm` 只能写自己的表）
- 不得在 handler 里直接写 SQL（统一放 `api.rs` / 后续拆 `repo/`）
- 不得把数据库行直接序列化（必须经 `CustomerVo`）
- 不得在前端做数据权限过滤（后端负责，前端只展示）
- 删除必须软删，物理删除由归档任务统一处理
