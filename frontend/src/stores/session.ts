// =============================================================================
// 会话状态（轻量 store）
//
// 为什么不用 Pinia：这里只有一个「当前用户 + 权限集」，用 ref 足够；
// 少一层依赖，AI 也更容易读懂。等状态变复杂（多模块共享）再引入 Pinia。
//
// 关键点：权限只存在于内存，刷新页面会重新拉取 —— 保证权限变更后能及时生效。
// =============================================================================

import { ref } from 'vue';
import { ApiError, http } from '../api/client';
import { queryClient } from '../api/query-client';
import { hasPermission } from '../perm-check';

/** 当前登录用户。 */
export interface CurrentUser {
  userId: string;
  tenantId: string;
  username: string;
  /** 权限点集合；`*` 表示超级管理员。 */
  permissions: string[];
  /** 是否必须先修改密码。 */
  mustChangePassword: boolean;
}

const currentUser = ref<CurrentUser | null>(null);
/** 并发去重：多个地方同时调用 loadSession 时只发一次请求。 */
let pending: Promise<CurrentUser> | null = null;
// clearSession 后旧请求仍可能成功返回；代次用于阻止旧身份覆盖新登录。
let generation = 0;

/** 加载当前用户并去重；已清除身份的旧请求以 401 失败，不写回用户或权限。 */
export function loadSession(): Promise<CurrentUser> {
  if (currentUser.value) {
    return Promise.resolve(currentUser.value);
  }
  if (!pending) {
    const started = generation;
    const task = http
      .get<CurrentUser>('/api/auth/me')
      .then((user) => {
        if (started !== generation) {
          throw new ApiError('sys.auth.unauthorized', 'HTTP 401', 401);
        }
        currentUser.value = user;
        return user;
      })
      .finally(() => {
        // 旧请求的 finally 不能清除新一轮登录正在等待的 Promise。
        if (pending === task) pending = null;
      });
    pending = task;
  }
  return pending;
}

/** 清空身份并使所有在途旧请求失效；下一次加载会重新请求服务端。 */
export function clearSession(): void {
  generation += 1;
  currentUser.value = null;
  pending = null;
  // 服务端缓存键并非都包含租户；身份变化必须清除全部查询，不能只清用户 ref。
  queryClient.clear();
}

/** 是否拥有某个权限点（规则见 `perm-check.ts`）。 */
export function hasPerm(code: string): boolean {
  return hasPermission(currentUser.value?.permissions, code);
}

/** 组合式入口。 */
export function useSession() {
  return { currentUser, loadSession, clearSession, hasPerm };
}
