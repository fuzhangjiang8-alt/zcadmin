// =============================================================================
// useDict：字典读取组合式函数
//
// 用法：
//   const { items, labelOf } = useDict('notice_type');
//   <a-select :options="items" />        // items 已是 [{label, value}]
//   labelOf('notice')                    // → '通知'
//
// 缓存策略：统一由 vue-query 去重和缓存，退出时随服务端状态清除，禁止跨身份复用。
// =============================================================================

import { computed } from 'vue';
import { useQuery } from '@tanstack/vue-query';
import { request } from '../api/client';
import { queryClient } from '../api/query-client';

/** 字典项（与后端 DictItemVo 对应）。 */
export interface DictItem {
  id: string;
  label: string;
  value: string;
  sort: number;
  status: string;
}

/** 缓存与组件共享相同选项；传递 signal，身份切换后旧查询不能重新写入缓存。 */
function dictOptions(code: string) {
  return {
    queryKey: ['system-dict', code] as const,
    queryFn: ({ signal }: { signal: AbortSignal }) =>
      request<DictItem[]>(`/api/system/dicts/${encodeURIComponent(code)}/items`, { signal }),
    staleTime: Infinity,
  };
}

/** 拉取字典项（带缓存与并发去重）。 */
export function loadDict(code: string): Promise<DictItem[]> {
  return queryClient.fetchQuery(dictOptions(code));
}

/** 清空缓存（退出登录、切换租户时调用）。 */
export function clearDictCache(): void {
  queryClient.removeQueries({ queryKey: ['system-dict'] });
}

/** 在组件里使用响应式字典；错误由查询状态管理，不产生未处理的 onMounted 拒绝。 */
export function useDict(code: string) {
  const query = useQuery(dictOptions(code));
  const items = computed(() => query.data.value ?? []);
  const loading = query.isLoading;

  /** 值 → 文案；找不到时原样返回，避免页面出现空白。 */
  function labelOf(value: string): string {
    return items.value.find((i) => i.value === value)?.label ?? value;
  }

  return { items, loading, labelOf };
}
