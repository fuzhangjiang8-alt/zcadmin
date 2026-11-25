// 服务端状态共享实例：应用和会话清理必须操作同一缓存，防止退出后复用上一租户的数据。
import { QueryClient } from '@tanstack/vue-query';

/** 统一请求缓存；退出或切换账号时 clear 会取消在途查询并清除查询与 mutation 状态。 */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: { staleTime: 30_000, refetchOnWindowFocus: false, retry: 1 },
  },
});
