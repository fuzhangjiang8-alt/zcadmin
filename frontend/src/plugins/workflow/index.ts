// =============================================================================
// Workflow 前端插件（可插拔企业审批流）
//
// 约定：
//   - 与后端 plugins/workflow 对应，路由路径 /workflow/requests；
//   - 若内部系统无需审批模块，从 frontendPlugins 移除或后端配置 DISABLED_PLUGINS=workflow 即可隐藏。
// =============================================================================

import type { FrontendPlugin } from '../types';

/** 审批流前端插件。 */
export const workflowPlugin: FrontendPlugin = {
  id: 'workflow',
  routes: [
    {
      path: 'workflow/requests',
      name: 'workflow-requests',
      component: () => import('./WorkflowView.vue'),
      meta: { perm: 'workflow:request:list' },
    },
  ],
};
