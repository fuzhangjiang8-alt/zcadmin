<template>
  <a-popover
    v-model:open="open"
    trigger="click"
    placement="bottomRight"
    @open-change="onOpen"
  >
    <template #content>
      <div class="notice-panel">
        <div class="notice-head">
          <span>{{ t('layout.notice') }}</span>
          <a-button type="link" size="small" @click="readAll">{{ t('layout.readAll') }}</a-button>
        </div>
        <a-spin :spinning="loading">
          <a-empty v-if="notices.length === 0" :description="t('layout.noNotice')" />
          <div
            v-for="n in notices"
            :key="n.id"
            class="notice-item"
            :class="{ unread: !n.isRead }"
            @click="readOne(n)"
          >
            <div class="notice-title">{{ n.title }}</div>
            <div class="notice-content">{{ n.content }}</div>
            <div class="notice-time">{{ n.publishedAt }}</div>
          </div>
        </a-spin>
      </div>
    </template>
    <a-badge :count="unread" :overflow-count="99">
      <a-button type="link">{{ t('layout.notice') }}</a-button>
    </a-badge>
  </a-popover>
</template>

<script setup lang="ts">
// =============================================================================
// 站内通知气泡
//
// 未读数每 30 秒轮询一次（简单够用）；通知失败不影响主流程。
// =============================================================================

import { onMounted, onUnmounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { http } from '../../api/client';

interface NoticeVo {
  id: string;
  title: string;
  content: string;
  level: string;
  publishedAt: string;
  isRead: boolean;
}

const { t } = useI18n();
const open = ref(false);
const loading = ref(false);
const unread = ref(0);
const notices = ref<NoticeVo[]>([]);
let timer: number | undefined;

async function loadUnread(): Promise<void> {
  try {
    const r = await http.get<{ count: number }>('/api/system/notices/unread-count');
    unread.value = r.count;
  } catch {
    // 通知失败不影响主流程
  }
}

async function loadNotices(): Promise<void> {
  loading.value = true;
  try {
    const r = await http.get<{ data: NoticeVo[] }>('/api/system/notices?page=1&size=10');
    notices.value = r.data;
  } finally {
    loading.value = false;
  }
}

function onOpen(value: boolean): void {
  if (value) void loadNotices();
}

async function readOne(n: NoticeVo): Promise<void> {
  if (n.isRead) return;
  try {
    await http.post(`/api/system/notices/${n.id}/read`);
    n.isRead = true;
    unread.value = Math.max(0, unread.value - 1);
  } catch {
    // 忽略单条失败
  }
}

async function readAll(): Promise<void> {
  try {
    await http.post('/api/system/notices/read-all');
    notices.value.forEach((n) => (n.isRead = true));
    unread.value = 0;
  } catch {
    // 忽略
  }
}

onMounted(() => {
  void loadUnread();
  timer = window.setInterval(loadUnread, 30_000);
});
onUnmounted(() => {
  if (timer) window.clearInterval(timer);
});
</script>

<style scoped>
.notice-panel {
  width: 320px;
  max-height: 420px;
  overflow-y: auto;
}

.notice-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-weight: 600;
  margin-bottom: 8px;
}

.notice-item {
  padding: 8px;
  border-radius: 6px;
  cursor: pointer;
}

.notice-item:hover {
  background: #f8fafc;
}

.notice-item.unread .notice-title::before {
  content: '';
  display: inline-block;
  width: 6px;
  height: 6px;
  margin-right: 6px;
  border-radius: 50%;
  background: #1677ff;
}

.notice-title {
  font-size: 13px;
  font-weight: 500;
}

.notice-content {
  font-size: 12px;
  color: #64748b;
  margin-top: 2px;
}

.notice-time {
  font-size: 11px;
  color: #94a3b8;
  margin-top: 4px;
}
</style>
