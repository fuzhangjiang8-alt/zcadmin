<template>
  <a-layout class="layout">
    <a-layout-sider
      v-model:collapsed="collapsed"
      collapsible
      theme="dark"
      :width="230"
      class="sider"
    >
      <div class="logo">
        <span class="logo-badge">Z</span>
        <span v-if="!collapsed" class="logo-text">{{ t('common.appName') }}</span>
      </div>

      <a-menu
        mode="inline"
        theme="dark"
        :selected-keys="selectedKeys"
        :open-keys="openKeys"
        @click="onMenuClick"
        @open-change="(keys: string[]) => (openKeys = keys)"
      >
        <template v-for="item in menus" :key="item.id">
          <a-sub-menu v-if="item.children.length > 0" :key="item.id">
            <template #title>{{ item.title }}</template>
            <a-menu-item v-for="child in item.children" :key="child.path">
              {{ child.title }}
            </a-menu-item>
          </a-sub-menu>
          <a-menu-item v-else :key="item.path">{{ item.title }}</a-menu-item>
        </template>
      </a-menu>
    </a-layout-sider>

    <a-layout>
      <a-layout-header class="header">
        <div class="header-left">
          <span class="page-title">{{ currentTitle }}</span>
        </div>
        <div class="header-right">
          <a-select v-model:value="locale" :options="localeOptions" size="small" style="width: 104px" />
          <NoticePopover />
          <a-divider type="vertical" class="header-divider" />
          <span class="user-chip">
            <span class="user-avatar">{{ userInitial }}</span>
            <span class="username">{{ username }}</span>
          </span>
          <a-button type="text" size="small" @click="pwdOpen = true">{{ t('common.changePassword') }}</a-button>
          <a-button type="text" size="small" @click="onLogout">{{ t('common.logout') }}</a-button>
        </div>
      </a-layout-header>

      <a-layout-content class="content">
        <router-view />
      </a-layout-content>
    </a-layout>
  </a-layout>

  <ChangePasswordModal v-model:open="pwdOpen" :must-change="mustChange" />
</template>

<script setup lang="ts">
// =============================================================================
// 后台布局（侧边栏 + 头部 + 内容区）
//
// 关键设计：菜单来自后端 `/api/system/menus`，前端不硬编码。
// 后端已按用户权限过滤，前端只负责渲染，避免「前端有菜单、接口 403」的错位。
// 通知与改密弹窗拆到 components/layout。
// =============================================================================

import { computed, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useI18n } from 'vue-i18n';
import { message } from 'ant-design-vue';
import { ApiError, http } from '../api/client';
import { clearSession, loadSession, useSession } from '../stores/session';
import ChangePasswordModal from '../components/layout/ChangePasswordModal.vue';
import NoticePopover from '../components/layout/NoticePopover.vue';

interface MenuNode {
  id: string;
  title: string;
  path: string;
  icon: string | null;
  children: MenuNode[];
}

const { t, locale } = useI18n();
const router = useRouter();
const route = useRoute();
const { currentUser } = useSession();

const menus = ref<MenuNode[]>([]);
const collapsed = ref(false);
const openKeys = ref<string[]>([]);
const pwdOpen = ref(false);

const localeOptions = [
  { label: t('layout.langZh'), value: 'zh-CN' },
  { label: 'English', value: 'en-US' },
];

const mustChange = computed(() => currentUser.value?.mustChangePassword ?? false);
watch(
  currentUser,
  (u) => {
    if (u?.mustChangePassword) pwdOpen.value = true;
  },
  { immediate: true },
);

const username = computed(() => currentUser.value?.username ?? '');
const userInitial = computed(() => username.value.charAt(0).toUpperCase() || 'U');
const selectedKeys = computed(() => [route.path]);
const currentTitle = computed(() => {
  const flat = flatten(menus.value);
  return flat.find((m) => m.path === route.path)?.title ?? '';
});

function flatten(nodes: MenuNode[]): MenuNode[] {
  return nodes.flatMap((n) => [n, ...flatten(n.children)]);
}

function onMenuClick({ key }: { key: string }): void {
  // 子菜单的 key 是 uuid（不导航），只有叶子节点是路径
  if (key.startsWith('/')) {
    void router.push(key);
  }
}

async function load(): Promise<void> {
  try {
    const [, menuTree] = await Promise.all([
      loadSession(),
      http.get<MenuNode[]>('/api/system/menus'),
    ]);
    menus.value = menuTree;
  } catch {
    message.error(t('layout.menuLoadFailed'));
  }
}

/** 服务端撤销成功后才清理身份；失败保留当前页面并提示重试，避免假退出。 */
async function onLogout(): Promise<void> {
  try {
    await http.post('/api/auth/logout');
    clearSession();
    await router.replace('/login');
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('common.logoutFailed'));
  }
}

onMounted(load);
</script>

<style scoped>
/* 整页锁定视口高度：侧边栏与顶栏固定，只有内容区滚动；菜单超高时侧边栏内部滚。 */
.layout {
  height: 100vh;
  overflow: hidden;
}

/* ---- 侧边栏：深色品牌主题 ---- */
.sider {
  background: var(--ad-sider-bg);
  box-shadow: 2px 0 16px rgb(15 23 42 / 25%);
}
/* 菜单超高时侧边栏内部滚动；隐藏滚动条避免占位，滚动仍可用 */
.sider :deep(.ant-layout-sider-children) {
  height: calc(100vh - 56px);
  overflow-y: auto;
  scrollbar-width: none;
}
.sider :deep(.ant-layout-sider-children)::-webkit-scrollbar {
  display: none;
}
.sider :deep(.ant-menu) {
  background: transparent;
  padding: 8px;
}
.sider :deep(.ant-menu-item),
.sider :deep(.ant-menu-submenu-title) {
  border-radius: 8px;
  margin: 2px 0;
  color: var(--ad-sider-item);
}
.sider :deep(.ant-menu-item:hover),
.sider :deep(.ant-menu-submenu-title:hover) {
  background: rgb(255 255 255 / 6%);
}
.sider :deep(.ant-menu-item-selected) {
  background: linear-gradient(90deg, var(--ad-color-primary), var(--ad-color-primary-deep));
  color: #fff;
  box-shadow: 0 4px 12px rgb(99 102 241 / 35%);
}

/* ---- 品牌区 ---- */
.logo {
  height: 56px;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 16px;
  border-bottom: 1px solid rgb(255 255 255 / 8%);
}
.logo-badge {
  width: 30px;
  height: 30px;
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  font-weight: 700;
  color: #fff;
  background: linear-gradient(135deg, var(--ad-color-primary), #22d3ee);
  box-shadow: 0 4px 10px rgb(99 102 241 / 45%);
}
.logo-text {
  font-size: 16px;
  font-weight: 600;
  color: #f1f5f9;
  letter-spacing: 0.5px;
}

/* ---- 顶栏：毛玻璃 ---- */
.header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 56px;
  padding: 0 24px;
  background: rgb(255 255 255 / 82%);
  backdrop-filter: blur(12px);
  border-bottom: 1px solid #e2e8f0;
  position: sticky;
  top: 0;
  z-index: 10;
}
.page-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--ad-color-text);
}
.header-right {
  display: flex;
  align-items: center;
  gap: 4px;
}
.header-divider {
  border-color: #e2e8f0;
}

/* ---- 用户胶囊 ---- */
.user-chip {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 34px;
  padding: 0 12px 0 4px;
  border-radius: 999px;
  background: #fff;
  border: 1px solid #e2e8f0;
  box-shadow: 0 1px 3px rgb(15 23 42 / 8%);
  transition: box-shadow 0.2s ease, border-color 0.2s ease;
}
.user-chip:hover {
  border-color: rgb(99 102 241 / 45%);
  box-shadow: 0 3px 10px rgb(99 102 241 / 18%);
}
.user-avatar {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  font-size: 12px;
  font-weight: 700;
  color: #fff;
  background: linear-gradient(135deg, var(--ad-color-primary), #0ea5e9);
  /* 内圈白描边让渐变更立体，外圈淡阴影避免「贴」在胶囊上 */
  box-shadow:
    inset 0 0 0 1.5px rgb(255 255 255 / 30%),
    0 1px 4px rgb(99 102 241 / 40%);
}
.username {
  font-size: 13px;
  color: #334155;
  font-weight: 500;
}

.content {
  padding: 24px;
  /* 唯一的滚动容器：顶栏 sticky 相对它生效，侧边栏因此保持固定 */
  height: calc(100vh - 56px);
  overflow-y: auto;
}
</style>
