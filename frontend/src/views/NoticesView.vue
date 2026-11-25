<template>
  <div class="page">
    <a-card :bordered="false" :title="t('notices.title')">
      <template #extra>
        <a-button v-perm="PERMS.NOTICE_ADD" type="primary" @click="openCreate">
          {{ t('notices.publish') }}
        </a-button>
      </template>

      <a-table
        :columns="columns"
        :data-source="notices"
        :loading="loading"
        row-key="id"
        :pagination="{
          current: page,
          pageSize: size,
          total,
          showSizeChanger: true,
          showTotal: (total: number) => t('notices.total', { n: total }),
        }"
        @change="onTableChange"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'level'">
            <a-tag :color="levelColor(record.level)">{{ levelText(record.level) }}</a-tag>
          </template>
          <template v-else-if="column.key === 'isRead'">
            <a-tag :color="record.isRead ? 'default' : 'blue'">
              {{ record.isRead ? t('notices.read') : t('notices.unread') }}
            </a-tag>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-space>
              <a-button type="link" size="small" @click="openDetail(record)">{{ t('notices.view') }}</a-button>
              <a-popconfirm :title="t('notices.deleteConfirm')" @confirm="onDelete(record)">
                <a-button v-perm="PERMS.NOTICE_ADD" type="link" danger size="small">{{ t('notices.delete') }}</a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>

    <a-modal v-model:open="detailOpen" :title="activeNotice?.title" :footer="null">
      <div style="white-space: pre-wrap; color: #334155">{{ activeNotice?.content }}</div>
    </a-modal>

    <a-modal
      v-model:open="createOpen"
      :title="t('notices.publishTitle')"
      :confirm-loading="submitting"
      :ok-text="t('notices.okPublish')"
      :cancel-text="t('notices.cancel')"
      @ok="submit"
    >
      <a-form ref="formRef" :model="form" :rules="rules" layout="vertical">
        <a-form-item :label="t('notices.titleLabel')" name="title">
          <a-input v-model:value="form.title" :placeholder="t('notices.titlePlaceholder')" />
        </a-form-item>
        <a-form-item :label="t('notices.level')" name="level">
          <a-select v-model:value="form.level" :options="levelOptions" />
        </a-form-item>
        <a-form-item :label="t('notices.content')" name="content">
          <a-textarea v-model:value="form.content" :rows="5" :placeholder="t('notices.contentPlaceholder')" />
        </a-form-item>
      </a-form>
    </a-modal>
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../permissions';
// =============================================================================
// 通知管理
//
// 发布是租户级广播；列表展示的是「当前用户」的已读状态。
// 正文按纯文本渲染（禁止 v-html），避免存储型 XSS。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { FormInstance, TablePaginationConfig } from 'ant-design-vue';
import { ApiError, http } from '../api/client';

interface NoticeVo {
  id: string;
  title: string;
  content: string;
  level: string;
  publishedAt: string;
  isRead: boolean;
}

interface PageResult<T> {
  data: T[];
  page: { page: number; size: number; total: number; hasNext: boolean };
}

const { t } = useI18n();
const queryClient = useQueryClient();
const page = ref(1);
const size = ref(20);

const query = useQuery({
  queryKey: computed(() => ['notices', page.value, size.value] as const),
  queryFn: () =>
    http.get<PageResult<NoticeVo>>(`/api/system/notices?page=${page.value}&size=${size.value}`),
  placeholderData: keepPreviousData,
});

const notices = computed(() => query.data.value?.data ?? []);
const total = computed(() => query.data.value?.page.total ?? 0);
const loading = computed(() => query.isFetching.value);

watch(query.error, (error) => {
  if (error) {
    message.error(error instanceof ApiError ? error.message : t('notices.loadFailed'));
  }
});

const columns = computed(() => [
  { title: t('notices.colTitle'), dataIndex: 'title', key: 'title' },
  { title: t('notices.colLevel'), dataIndex: 'level', key: 'level', width: 100 },
  { title: t('notices.colStatus'), dataIndex: 'isRead', key: 'isRead', width: 90 },
  { title: t('notices.colPublished'), dataIndex: 'publishedAt', key: 'publishedAt', width: 190 },
  { title: t('notices.colActions'), key: 'actions', width: 140 },
]);

const levelOptions = computed(() => [
  { label: t('notices.levelInfo'), value: 'info' },
  { label: t('notices.levelSuccess'), value: 'success' },
  { label: t('notices.levelWarning'), value: 'warning' },
  { label: t('notices.levelError'), value: 'error' },
]);

function levelText(level: string): string {
  return levelOptions.value.find((o) => o.value === level)?.label ?? level;
}

function levelColor(level: string): string {
  return { info: 'blue', success: 'green', warning: 'orange', error: 'red' }[level] ?? 'default';
}

function onTableChange(pagination: TablePaginationConfig): void {
  page.value = pagination.current ?? 1;
  size.value = pagination.pageSize ?? 20;
}

const detailOpen = ref(false);
const activeNotice = ref<NoticeVo | null>(null);

async function openDetail(record: NoticeVo): Promise<void> {
  activeNotice.value = record;
  detailOpen.value = true;
  if (!record.isRead) {
    try {
      await http.post(`/api/system/notices/${record.id}/read`);
      void queryClient.invalidateQueries({ queryKey: ['notices'] });
    } catch {
      // 标记已读失败不影响查看正文
    }
  }
}

const deleteMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/system/notices/${id}`),
  onSuccess: () => {
    message.success(t('notices.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['notices'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('notices.deleteFailed'));
  },
});

function onDelete(record: NoticeVo): void {
  deleteMutation.mutate(record.id);
}

// ---- 发布 ----
const createOpen = ref(false);
const submitting = ref(false);
const formRef = ref<FormInstance>();
const form = reactive({ title: '', level: 'info', content: '' });
const rules = {
  title: [{ required: true, max: 200, message: t('notices.titleRule') }],
  content: [{ required: true, message: t('notices.contentRequired') }],
};

function openCreate(): void {
  form.title = '';
  form.level = 'info';
  form.content = '';
  createOpen.value = true;
}

const publishMutation = useMutation({
  mutationFn: () =>
    http.post<NoticeVo>('/api/system/notices', {
      title: form.title.trim(),
      level: form.level,
      content: form.content.trim(),
    }),
  onSuccess: () => {
    message.success(t('notices.published'));
    createOpen.value = false;
    void queryClient.invalidateQueries({ queryKey: ['notices'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('notices.publishFailed'));
  },
  onSettled: () => {
    submitting.value = false;
  },
});

async function submit(): Promise<void> {
  try {
    await formRef.value?.validate();
  } catch {
    return;
  }
  submitting.value = true;
  publishMutation.mutate();
}
</script>

<style scoped>
.page {
  padding: 0;
}
</style>
