# demo 插件 · 领域上下文

> 本文件是给「人 + AI」看的业务内存。写代码前先读它，不要靠猜。
> 结构固定：术语 → 表 → 权限点 → 接口 → 事件 → 状态机 → 禁止事项。

---

## 1. 领域术语

| 术语 | 含义 |
|---|---|
| 示例项（Demo Item） | 仅用于验证插件契约的占位实体，**生产环境不会存在** |
| 软删 | 设置 `deleted_at`，查询默认过滤；物理删除由归档任务处理 |

## 2. 数据表

| 表 | 说明 | 关键约束 |
|---|---|---|
| `demo.demo_item` | 示例实体 | `id` 主键（UUIDv7）；`name` 1~64 字符 |

## 3. 权限点

| 权限点 | 说明 |
|---|---|
| `demo:item:list` | 列表查询 |
| `demo:item:add` | 新增 |

> M0 尚未接入 RBAC，接口暂未强制校验；M1 会由 `require_perm!` 统一拦截。

## 4. 接口

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/demo/ping` | 连通性探测，无需登录 |
| GET | `/api/demo/items?page=1&size=20` | 分页列表 |
| POST | `/api/demo/items` | 创建 |
| GET | `/api/demo/items/{id}` | 详情 |
| DELETE | `/api/demo/items/{id}` | 软删 |

## 5. 事件

| 方向 | 事件 | 说明 |
|---|---|---|
| 发布 | `demo.item.created` | M1 接入 outbox 后生效 |
| 订阅 | 无 | — |

## 6. 状态机

```
（创建） → active → deleted（软删，不可逆）
```

## 7. 禁止事项

- 不得查询其他插件的 schema
- 不得在 handler 里直接写 SQL（应放 `repo/`）
- 不得把 `DemoItem` 直接用作创建请求体（防 mass assignment）
- 不得在日志里打印 `note` 全文（可能含用户敏感信息）
