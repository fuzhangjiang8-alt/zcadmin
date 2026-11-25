// =============================================================================
// 权限判断（纯函数）
//
// 为什么单独拆出来：`hasPerm` 依赖会话状态（Vue ref），
// 纯函数可以在 Node 里直接跑单测，不用挂载组件、不用 mock HTTP。
// 规则与后端 `CurrentUser::has_permission` 保持一致：
//   - `*` 视为超管，放行一切；
//   - 其余按权限点精确匹配。
// =============================================================================

/** 判断权限集合是否包含某个权限点。 */
export function hasPermission(
  list: readonly string[] | null | undefined,
  code: string,
): boolean {
  if (!code) {
    return false;
  }
  const perms = list ?? [];
  return perms.includes('*') || perms.includes(code);
}
