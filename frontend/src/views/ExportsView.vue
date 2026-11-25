<template>
  <div class="page">
    <a-card :bordered="false" :title="t('exports.title')">
      <template #extra>
        <a-space>
          <a-tag v-if="hasRunning" color="processing">{{ t('exports.running') }}</a-tag>
          <a-button :loading="loading" @click="reload">{{ t('exports.refresh') }}</a-button>
        </a-space>
      </template>

      <a-table
        :columns="columns"
        :data-source="tasks"
        :loading="loading"
        row-key="id"
        :pagination="{
          current: page,
          pageSize: size,
          total,
          showSizeChanger: true,
          showTotal: (total: number) => t('exports.total', { n: total }),
        }"
        @change="onTableChange"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'kind'">{{ kindText(record.kind) }}</template>
          <template v-else-if="column.key === 'status'">
            <a-tag :color="statusColor(record.status)">{{ statusText(record.status) }}</a-tag>
          </template>
          <template v-else-if="column.key === 'error'">
            <span class="error">{{ record.error ?? '-' }}</span>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-space>
              <a-button
                v-if="record.status === 'success'"
                type="link"
                size="small"
                @click="download(record)"
              >
                {{ t('exports.download') }}
              </a-button>
              <a-button type="link" size="small" @click="openDetail(record)">{{ t('exports.detail') }}</a-button>
              <a-popconfirm :title="t('exports.deleteConfirm')" @confirm="onDelete(record)">
                <a-button type="link" size="small" danger>{{ t('exports.delete') }}</a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>

    <a-modal v-model:open="filterOpen" :title="t('exports.filterTitle')" :footer="null">
      <pre class="filter-json">{{ filterText }}</pre>
    </a-modal>
  </div>
</template>

<script setup lang="ts">
// =============================================================================
// 导出任务列表
//
// 异步导出的收尾页面：关闭触发页面后，仍能在这里找到任务、下载或删除。
// 有 pending/running 任务时每 3 秒自动刷新，全部结束后停止轮询。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { TablePaginationConfig } from 'ant-design-vue';
import { ApiError, http } from '../api/client';

interface ExportTaskVo {
  id: string;
  kind: string;
  keyword: string | null;
  status: string;
  fileName: string | null;
  totalRows: number | null;
  error: string | null;
  createdAt: string;
  finishedAt: string | null;
  filters: Record<string, unknown> | null;
}

interface PageResult<T> {
  data: T[];
  page: { page: number; size: number; total: number; hasNext: boolean };
}

const { t } = useI18n();
const queryClient = useQueryClient();
const page = ref(1);
const size = ref(20);
const filterOpen = ref(false);
const filterText = ref('');
const hasRunning = ref(false);

const query = useQuery({
  queryKey: computed(() => ['export-tasks', page.value, size.value] as const),
  queryFn: () =>
    http.get<PageResult<ExportTaskVo>>(
      `/api/system/export-tasks?page=${page.value}&size=${size.value}`,
    ),
  placeholderData: keepPreviousData,
  // 有排队/生成中的任务时每 3 秒轮询，全部结束后自动停止
  refetchInterval: computed(() => (hasRunning.value ? 3000 : false)),
});

const tasks = computed(() => query.data.value?.data ?? []);
const total = computed(() => query.data.value?.page.total ?? 0);
const loading = computed(() => query.isFetching.value);

watch(
  () => query.data.value?.data ?? [],
  (rows) => {
    hasRunning.value = rows.some((task) => task.status === 'pending' || task.status === 'running');
  },
  { immediate: true },
);

watch(query.error, (error) => {
  if (error) {
    message.error(error instanceof ApiError ? error.message : t('exports.loadFailed'));
  }
});

const columns = computed(() => [
  { title: t('exports.colKind'), dataIndex: 'kind', key: 'kind', width: 140 },
  { title: t('exports.colKeyword'), dataIndex: 'keyword', key: 'keyword', width: 140 },
  { title: t('exports.colStatus'), dataIndex: 'status', key: 'status', width: 100 },
  { title: t('exports.colRows'), dataIndex: 'totalRows', key: 'totalRows', width: 90 },
  { title: t('exports.colCreated'), dataIndex: 'createdAt', key: 'createdAt', width: 180 },
  { title: t('exports.colFinished'), dataIndex: 'finishedAt', key: 'finishedAt', width: 180 },
  { title: t('exports.colError'), dataIndex: 'error', key: 'error' },
  { title: t('exports.colActions'), key: 'actions', width: 140 },
]);

const kindText = (kind: string): string =>
  ({ 'system.audit': t('exports.kindAudit'), 'system.users': t('exports.kindUsers') })[kind] ?? kind;

const statusText = (status: string): string =>
  ({ pending: t('exports.statusPending'), running: t('exports.statusRunning'), success: t('exports.statusSuccess'), failed: t('exports.statusFailed') })[status] ?? status;

const statusColor = (status: string): string =>
  ({ pending: 'default', running: 'processing', success: 'green', failed: 'red' })[status] ??
  'default';

function reload(): void {
  void query.refetch();
}

function onTableChange(pagination: TablePaginationConfig): void {
  page.value = pagination.current ?? 1;
  size.value = pagination.pageSize ?? 20;
}

function openDetail(task: ExportTaskVo): void {
  filterOpen.value = true;
  filterText.value = task.filters ? JSON.stringify(task.filters, null, 2) : t('exports.noFilters');
}

function download(task: ExportTaskVo): void {
  window.open(`/api/system/export-tasks/${task.id}/download`, '_blank');
}

const deleteMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/system/export-tasks/${id}`),
  onSuccess: () => {
    message.success(t('exports.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['export-tasks'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('exports.deleteFailed'));
  },
});

function onDelete(task: ExportTaskVo): void {
  deleteMutation.mutate(task.id);
}
</script>

<style scoped>
.filter-json {
  margin: 0;
  max-height: 420px;
  overflow: auto;
  font-size: 12px;
  background: #f8fafc;
  padding: 12px;
  border-radius: 6px;
}
</style>

<style scoped>
.page {
  padding: 0;
}

.error {
  color: #dc2626;
  font-size: 12px;
}
</style>
