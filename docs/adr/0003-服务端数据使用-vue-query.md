# ADR-0003：服务端数据使用 @tanstack/vue-query

- 状态：已采纳（2026-11-02）
- 相关代码：`frontend/src/main.ts`、`frontend/src/views/PostsView.vue`
- 相关规范：`AGENTS.md` 铁律 13「服务端数据用 @tanstack/vue-query，禁止塞进 Pinia」

## 背景

列表页目前普遍是「`ref` + 手写 `reload()` + 手动 loading/错误处理」：

1. 每个页面重复一套加载/错误/刷新逻辑，容易漏掉错误提示；
2. 写操作后要记得手动重新拉列表，漏掉就展示脏数据；
3. 缓存在页面间不共享，来回切换重复请求。

## 决策

引入 **`@tanstack/vue-query`**，统一管理服务端状态：

- `main.ts` 注册 `VueQueryPlugin`，默认 `staleTime: 30s`、`refetchOnWindowFocus: false`、`retry: 1`；
- 列表读取用 `useQuery`，queryKey 形如 `['posts', page, size, keyword]`；
- 写操作用 `useMutation`，成功后 `invalidateQueries` 对应前缀，由缓存层触发刷新；
- 客户端状态（弹窗开关、表单草稿）仍用 `ref/reactive`，不放进 query。

## 备选方案

| 方案 | 不选原因 |
|---|---|
| 继续手写 ref + reload | 重复逻辑多、易漏刷新，且无法跨页缓存 |
| Pinia 存列表 | 与服务端状态混在一起，需要自己处理失效/竞态 |
| SWR 风格自研 | 并发去重、重试、失效都要自己写 |

## 影响

- 新增前端依赖 `@tanstack/vue-query`（约 13KB gzip）；
- 迁移按页面分批：先 `PostsView` 作范式，其余列表页逐轮替换；
- 未迁移页面短期仍手写，属已知过渡状态。

## 回滚

删除 `main.ts` 的插件注册与 `src/views` 中的 `useQuery` 用法，恢复为 `ref + reload`；
按页面分批迁移可将回滚范围限制在单页。
