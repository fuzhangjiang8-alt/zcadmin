// =============================================================================
// 前端入口
//
// 职责：注册组件、指令、路由，挂载应用。业务逻辑一律不写在这里。
//
// 组件注册策略：**按需注册**（不是 `app.use(Antd)` 全量）。
// 全量引入会让首屏多出 ~400KB gzip；按需注册只打进真正用到的组件。
// 新增页面用到新组件时，在这里补一行 —— 这是刻意保留的显式成本。
// =============================================================================

import { createApp } from 'vue';
import type { Plugin } from 'vue';
import {
  Alert,
  Badge,
  Button,
  Card,
  Checkbox,
  Col,
  ConfigProvider,
  DatePicker,
  Descriptions,
  Divider,
  Drawer,
  Empty,
  Form,
  Input,
  InputNumber,
  Layout,
  Menu,
  Modal,
  Popconfirm,
  Popover,
  Progress,
  Radio,
  Row,
  Select,
  Space,
  Spin,
  Statistic,
  Switch,
  Table,
  Tag,
  Upload,
} from 'ant-design-vue';
import 'ant-design-vue/dist/reset.css';
import 'dayjs/locale/zh-cn';

import App from './App.vue';
import { router } from './router';
import { i18n } from './i18n';
import { VueQueryPlugin } from '@tanstack/vue-query';
import { queryClient } from './api/query-client';
import { vPerm } from './directives/perm';
import './styles.css';

const app = createApp(App);

// 按需注册：每个组件自带其子组件（如 Layout 会注册 Header/Sider/Content）
const antdComponents: Plugin[] = [
  Alert,
  Badge,
  Button,
  Card,
  Checkbox,
  Col,
  ConfigProvider,
  DatePicker,
  Descriptions,
  Divider,
  Drawer,
  Empty,
  Form,
  Input,
  InputNumber,
  Layout,
  Menu,
  Modal,
  Popconfirm,
  Popover,
  Progress,
  Radio,
  Row,
  Select,
  Space,
  Spin,
  Statistic,
  Switch,
  Table,
  Tag,
  Upload,
];
antdComponents.forEach((component) => app.use(component));

// 全局指令：按钮级权限
app.directive('perm', vPerm);

app.use(router);

// 服务端状态统一走 vue-query（见 docs/adr/0003）
app.use(VueQueryPlugin, { queryClient });
app.use(i18n);
app.mount('#app');
