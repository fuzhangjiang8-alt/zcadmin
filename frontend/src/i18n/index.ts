// =============================================================================
// i18n 实例
//
// 决策见 docs/adr/0002-前端国际化方案.md：
// - Composition API 模式（legacy: false）；
// - 默认中文，英文作为第二语言；
// - 语言只存内存，刷新后回到默认（避免 localStorage 与账号偏好不一致）。
// =============================================================================

import { createI18n } from 'vue-i18n';
import zhCN from './zh-CN';
import enUS from './en-US';

export const i18n = createI18n({
  legacy: false,
  locale: 'zh-CN',
  fallbackLocale: 'zh-CN',
  messages: {
    'zh-CN': zhCN,
    'en-US': enUS,
  },
});
