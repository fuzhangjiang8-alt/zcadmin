// =============================================================================
// 前端插件注册表
//
// 新增一个业务插件：
//   1. src/plugins/<id>/index.ts 导出 FrontendPlugin
//   2. 这里 import 并加入数组
//   3. 后端插件用 Plugin::menus() 声明菜单（同一个 path）
//
// 只要这两边 path 对齐，菜单点开就能进到页面。
// =============================================================================

import type { FrontendPlugin } from './types';
import { crmPlugin } from './crm';
import { workflowPlugin } from './workflow';

/** 已注册的前端插件（支持按系统需求拔插）。 */
export const frontendPlugins: FrontendPlugin[] = [crmPlugin, workflowPlugin];
