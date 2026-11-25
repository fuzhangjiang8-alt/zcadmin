// =============================================================================
// CRM 前端插件
//
// 约定：
//   - 页面组件放在本目录内，不引用其他插件的组件；
//   - 可以引用 @/ 下内核提供的能力（api client、session、指令）；
//   - 路由 name 统一加插件前缀（crm-），避免跨插件重名；
//   - 每条路由声明 `meta.perm`，由内核路由守卫统一校验（后端仍会 403 兜底）。
// =============================================================================

import type { FrontendPlugin } from '../types';

/** CRM 插件。 */
export const crmPlugin: FrontendPlugin = {
  id: 'crm',
  routes: [
    {
      path: 'crm/dashboard',
      name: 'crm-dashboard',
      component: () => import('./DashboardView.vue'),
      meta: { perm: 'crm:customer:list' },
    },
    {
      path: 'crm/customers',
      name: 'crm-customers',
      component: () => import('./CustomersView.vue'),
      meta: { perm: 'crm:customer:list' },
    },
    {
      path: 'crm/customers/:id',
      name: 'crm-customer-detail',
      component: () => import('./CustomerDetailView.vue'),
      meta: { perm: 'crm:customer:list' },
    },
    {
      path: 'crm/contacts',
      name: 'crm-contacts',
      component: () => import('./ContactsView.vue'),
      meta: { perm: 'crm:contact:list' },
    },
    {
      path: 'crm/opportunities',
      name: 'crm-opportunities',
      component: () => import('./OpportunitiesView.vue'),
      meta: { perm: 'crm:opportunity:list' },
    },
    {
      path: 'crm/opportunities/:id',
      name: 'crm-opportunity-detail',
      component: () => import('./OpportunityDetailView.vue'),
      meta: { perm: 'crm:opportunity:list' },
    },
    {
      path: 'crm/pipeline',
      name: 'crm-pipeline',
      component: () => import('./PipelineView.vue'),
      meta: { perm: 'crm:opportunity:list' },
    },
    {
      path: 'crm/contracts',
      name: 'crm-contracts',
      component: () => import('./ContractsView.vue'),
      meta: { perm: 'crm:contract:list' },
    },
    {
      path: 'crm/contracts/:id',
      name: 'crm-contract-detail',
      component: () => import('./ContractDetailView.vue'),
      meta: { perm: 'crm:contract:list' },
    },
    {
      path: 'crm/payments',
      name: 'crm-payments',
      component: () => import('./PaymentsView.vue'),
      meta: { perm: 'crm:payment:list' },
    },
  ],
};
