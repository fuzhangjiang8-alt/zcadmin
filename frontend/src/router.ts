// =============================================================================
// 路由表与守卫
//
// 结构：
//   /login            独立页面（无布局）
//   /403              无权限页（无布局，避免泄露菜单结构）
//   /                 后台布局（LayoutView），子路由渲染在内容区
//     ├─ ''           工作台
//     ├─ system/users 用户管理
//     └─ system/roles 角色管理
//
// 注意：侧边栏菜单来自后端 `/api/system/menus`，这里只维护"路径 → 组件"的映射。
// 后端菜单里出现的路径必须在这里存在，否则会 404（M1-b 用手工映射，M2 会改为生成）。
//
// 权限：每条路由用 `meta.perm` 声明进入所需权限点，守卫在会话加载后比对；
//       不通过时跳 `/403`。这只是**体验层**兜底，真正的边界仍在后端 require_perm。
// =============================================================================

import { createRouter, createWebHistory } from 'vue-router';
import type { RouteRecordRaw } from 'vue-router';
import { loadSession, hasPerm } from './stores/session';
import { frontendPlugins } from './plugins';

/** 带权限点的路由记录。 */
type PermRoute = RouteRecordRaw & { meta?: { perm?: string } };

const systemRoutes: PermRoute[] = [
  { path: '', name: 'home', component: () => import('./views/HomeView.vue') },
  {
    path: 'system/users',
    name: 'system-users',
    component: () => import('./views/UsersView.vue'),
    meta: { perm: 'system:user:list' },
  },
  {
    path: 'system/depts',
    name: 'system-depts',
    component: () => import('./views/DeptView.vue'),
  },
  {
    path: 'system/audit',
    name: 'system-audit',
    component: () => import('./views/AuditLogsView.vue'),
    meta: { perm: 'system:audit:list' },
  },
  {
    path: 'system/sessions',
    name: 'system-sessions',
    component: () => import('./views/SessionsView.vue'),
    meta: { perm: 'system:session:list' },
  },
  {
    path: 'system/configs',
    name: 'system-configs',
    component: () => import('./views/ConfigsView.vue'),
    meta: { perm: 'system:config:list' },
  },
  {
    path: 'system/roles',
    name: 'system-roles',
    component: () => import('./views/RolesView.vue'),
    meta: { perm: 'system:role:list' },
  },
  {
    path: 'system/dicts',
    name: 'system-dicts',
    component: () => import('./views/DictsView.vue'),
    meta: { perm: 'system:dict:list' },
  },
  {
    path: 'system/files',
    name: 'system-files',
    component: () => import('./views/FilesView.vue'),
    meta: { perm: 'system:file:list' },
  },
  {
    path: 'system/notices',
    name: 'system-notices',
    component: () => import('./views/NoticesView.vue'),
  },
  {
    path: 'system/menus',
    name: 'system-menus',
    component: () => import('./views/MenusView.vue'),
    meta: { perm: 'system:menu:list' },
  },
  {
    path: 'system/exports',
    name: 'system-exports',
    component: () => import('./views/ExportsView.vue'),
    meta: { perm: 'system:export:list' },
  },
  {
    path: 'system/tenants',
    name: 'system-tenants',
    component: () => import('./views/TenantsView.vue'),
    meta: { perm: 'system:tenant:list' },
  },
  {
    path: 'system/posts',
    name: 'system-posts',
    component: () => import('./views/PostsView.vue'),
    meta: { perm: 'system:post:list' },
  },
  {
    path: 'system/ops',
    name: 'system-ops',
    component: () => import('./views/OpsMonitorView.vue'),
    meta: { perm: 'system:audit:list' },
  },
];

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/login',
      name: 'login',
      component: () => import('./views/LoginView.vue'),
      meta: { public: true },
    },
    {
      path: '/403',
      name: 'forbidden',
      component: () => import('./views/ForbiddenView.vue'),
      meta: { public: true },
    },
    {
      path: '/',
      component: () => import('./views/LayoutView.vue'),
      children: [
        ...systemRoutes,
        // 业务插件路由（CRM / IM / AI / IoT ...）
        ...frontendPlugins.flatMap((plugin) => plugin.routes),
      ],
    },
    { path: '/:pathMatch(.*)*', redirect: '/' },
  ],
});

/**
 * 全局前置守卫：先确认会话有效，再校验路由权限点。
 *
 * 这里调用 /api/auth/me 而不是检查 localStorage —— 令牌在 HttpOnly Cookie 里，
 * 前端根本读不到，只能问后端「我是谁」。
 * 权限不通过跳 /403：直接输入 URL 也看不到页面壳（后端仍会 403 兜底）。
 */
router.beforeEach(async (to) => {
  if (to.meta.public === true) {
    return true;
  }

  try {
    await loadSession();
  } catch {
    return { name: 'login', query: { redirect: to.fullPath } };
  }

  const perm = to.meta.perm;
  if (typeof perm === 'string' && perm.length > 0 && !hasPerm(perm)) {
    return { name: 'forbidden' };
  }
  return true;
});

declare module 'vue-router' {
  interface RouteMeta {
    /** 访问该路由所需的权限点。 */
    perm?: string;
    /** 无需登录。 */
    public?: boolean;
  }
}
