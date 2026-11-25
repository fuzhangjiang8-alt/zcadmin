// =============================================================================
// v-perm 指令：按钮级权限
//
// 用法：
//   <a-button v-perm="'system:user:add'">新建用户</a-button>
//
// 行为：无权限时**直接移除 DOM 节点**，而不是隐藏（隐藏仍可通过 devtools 改回来）。
//
// 重要：前端权限只影响"看不看得见"，不是安全边界。
//       后端每个写接口都必须独立校验（见 kernel/src/system.rs 的 require_perm）。
// =============================================================================

import type { Directive } from 'vue';
import { hasPerm } from '../stores/session';

export const vPerm: Directive<HTMLElement, string> = {
  mounted(el, binding) {
    const code = binding.value;
    if (code && !hasPerm(code)) {
      el.parentNode?.removeChild(el);
    }
  },
};
