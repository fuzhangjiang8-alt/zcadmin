<template>
  <div class="page">
    <a-card :bordered="false" :title="t('files.title')">
      <template #extra>
        <a-space>
          <a-input-search
            v-model:value="searchText"
            :placeholder="t('files.searchPlaceholder')"
            allow-clear
            style="width: 220px"
            @search="onSearch"
          />
          <a-upload
            v-perm="PERMS.FILE_UPLOAD"
            :custom-request="upload"
            :show-upload-list="false"
            :max-count="1"
          >
            <a-button v-perm="PERMS.FILE_UPLOAD" type="primary" :loading="uploading">{{ t('files.upload') }}</a-button>
          </a-upload>
        </a-space>
      </template>

      <a-alert
        type="info"
        show-icon
        :message="t('files.info')"
        style="margin-bottom: 16px"
      />

      <a-table
        :columns="columns"
        :data-source="files"
        :loading="loading"
        row-key="id"
        :pagination="{
          current: page,
          pageSize: size,
          total,
          showSizeChanger: true,
          showTotal: (total: number) => t('files.total', { n: total }),
        }"
        @change="onTableChange"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'size'">{{ formatSize(record.sizeBytes) }}</template>
          <template v-else-if="column.key === 'actions'">
            <a-space>
              <a :href="`/api/system/files/${record.id}/content`" target="_blank">{{ t('files.download') }}</a>
              <a-popconfirm :title="t('files.deleteConfirm')" @confirm="onDelete(record.id)">
                <a-button v-perm="PERMS.FILE_DELETE" type="link" size="small" danger>
                  {{ t('files.delete') }}
                </a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../permissions';
// =============================================================================
// 文件管理
//
// 上传走 multipart 直传后端（M3 简化版）；接入 S3/MinIO 后改成预签名直传，
// 前端只需替换 upload() 的实现，其余不变。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { TablePaginationConfig } from 'ant-design-vue';

/** AntD Upload 的最小请求参数（避免深层导入其内部类型）。 */
interface UploadRequestOption {
  file: File;
  onSuccess?: (body: unknown) => void;
  onError?: (err: Error) => void;
}
import { ApiError, http } from '../api/client';

interface FileVo {
  id: string;
  name: string;
  mime: string;
  sizeBytes: number;
  createdAt: string;
}

interface PageResult<T> {
  data: T[];
  page: { page: number; size: number; total: number; hasNext: boolean };
}

const { t } = useI18n();
const queryClient = useQueryClient();
const page = ref(1);
const size = ref(20);
const searchText = ref('');
const keyword = ref('');
const uploading = ref(false);

const query = useQuery({
  queryKey: computed(() => ['files', page.value, size.value, keyword.value] as const),
  queryFn: () => {
    const params = new URLSearchParams({ page: String(page.value), size: String(size.value) });
    if (keyword.value.trim()) params.set('keyword', keyword.value.trim());
    return http.get<PageResult<FileVo>>(`/api/system/files?${params.toString()}`);
  },
  placeholderData: keepPreviousData,
});

function onSearch(): void {
  page.value = 1;
  keyword.value = searchText.value;
}

const files = computed(() => query.data.value?.data ?? []);
const total = computed(() => query.data.value?.page.total ?? 0);
const loading = computed(() => query.isFetching.value);

watch(query.error, (error) => {
  if (error) {
    message.error(error instanceof ApiError ? error.message : t('files.loadFailed'));
  }
});

const columns = computed(() => [
  { title: t('files.colName'), dataIndex: 'name', key: 'name' },
  { title: t('files.colType'), dataIndex: 'mime', key: 'mime', width: 220 },
  { title: t('files.colSize'), dataIndex: 'sizeBytes', key: 'size', width: 110 },
  { title: t('files.colCreated'), dataIndex: 'createdAt', key: 'createdAt', width: 190 },
  { title: t('files.colActions'), key: 'actions', width: 140 },
]);

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function onTableChange(pagination: TablePaginationConfig): void {
  page.value = pagination.current ?? 1;
  size.value = pagination.pageSize ?? 20;
}

/** 自定义上传：直接 POST multipart，成功后失效列表缓存。 */
async function upload(options: UploadRequestOption): Promise<void> {
  const file = options.file as File;
  const form = new FormData();
  form.append('file', file);

  uploading.value = true;
  try {
    const response = await fetch('/api/system/files/upload', {
      method: 'POST',
      body: form,
      credentials: 'include',
    });
    if (!response.ok) {
      const problem = await response.json().catch(() => ({}));
      throw new ApiError(problem.code ?? 'upload.failed', problem.detail ?? t('files.uploadFailed'), response.status);
    }
    message.success(t('files.uploaded'));
    options.onSuccess?.(await response.json());
    void queryClient.invalidateQueries({ queryKey: ['files'] });
  } catch (error) {
    const err = error instanceof ApiError ? error : new ApiError('upload.failed', t('files.uploadFailed'), 0);
    message.error(err.message);
    options.onError?.(err as never);
  } finally {
    uploading.value = false;
  }
}

const deleteMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/system/files/${id}`),
  onSuccess: () => {
    message.success(t('files.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['files'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('files.deleteFailed'));
  },
});

function onDelete(id: string): void {
  deleteMutation.mutate(id);
}
</script>

<style scoped>
.page {
  padding: 0;
}
</style>
