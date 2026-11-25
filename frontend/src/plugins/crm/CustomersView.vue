<template>
  <div class="page">
    <a-card :bordered="false" :title="t('crm.customersTitle')">
      <template #extra>
        <a-space>
          <a-input-search
            v-model:value="searchText"
            :placeholder="t('crm.searchCustomer')"
            allow-clear
            style="width: 240px"
            @search="onSearch"
          />
          <a-button v-perm="PERMS.CUSTOMER_EXPORT" @click="onExport">{{ t('crm.exportBtn') }}</a-button>
          <a-upload
            v-perm="PERMS.CUSTOMER_IMPORT"
            :custom-request="onImport"
            :show-upload-list="false"
            accept=".xlsx"
          >
            <a-button :loading="importing">{{ t('crm.importBtn') }}</a-button>
          </a-upload>
          <a-button v-perm="PERMS.CUSTOMER_ADD" type="primary" @click="createOpen = true">
            {{ t('crm.createCustomer') }}
          </a-button>
        </a-space>
      </template>

      <a-table
        :columns="columns"
        :data-source="customers"
        :loading="loading"
        row-key="id"
        :pagination="{
          current: page,
          pageSize: size,
          total,
          showSizeChanger: true,
          showTotal: (total: number) => t('crm.totalCount', { n: total }),
        }"
        @change="onTableChange"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'name'">
            <router-link :to="{ name: 'crm-customer-detail', params: { id: record.id } }">
              {{ record.name }}
            </router-link>
          </template>
          <template v-else-if="column.key === 'level'">
            <a-tag>{{ levelText(record.level) }}</a-tag>
          </template>
          <template v-else-if="column.key === 'status'">
            <a-tag :color="statusColor(record.status)">{{ statusText(record.status) }}</a-tag>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-popconfirm :title="t('crm.deleteConfirmCustomer')" @confirm="onDelete(record.id)">
              <a-button v-perm="PERMS.CUSTOMER_DELETE" type="link" size="small" danger>
                {{ t('crm.delete') }}
              </a-button>
            </a-popconfirm>
          </template>
        </template>
      </a-table>
    </a-card>

    <CustomerCreateModal v-model:open="createOpen" @saved="reload" />

    <!-- 导入结果 -->
    <a-modal v-model:open="importOpen" :title="t('crm.importResultTitle')" :footer="null">
      <a-descriptions :column="3" size="small">
        <a-descriptions-item :label="t('crm.totalRows')">{{ importResult?.total ?? 0 }}</a-descriptions-item>
        <a-descriptions-item :label="t('crm.successCount')">{{ importResult?.success ?? 0 }}</a-descriptions-item>
        <a-descriptions-item :label="t('crm.failedCount')">{{ importResult?.failed ?? 0 }}</a-descriptions-item>
      </a-descriptions>

      <a-table
        v-if="importResult && importResult.errors.length > 0"
        size="small"
        style="margin-top: 12px"
        :pagination="false"
        :data-source="importResult.errors"
        :columns="[
          { title: t('crm.rowNo'), dataIndex: 'row', key: 'row', width: 80 },
          { title: t('crm.reason'), dataIndex: 'message', key: 'message' },
        ]"
        row-key="row"
      />
    </a-modal>
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../../permissions';
// =============================================================================
// 客户管理（CRM 插件）
//
// 这是第一个业务插件页面，结构与 UsersView 一致：
// 工具栏 + 服务端分页表格 + 弹窗表单。
// 权限由 v-perm 控制显示，后端仍会独立校验（双重保险）。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { TablePaginationConfig } from 'ant-design-vue';
import { ApiError, http } from '../../api/client';

interface CustomerVo {
  id: string;
  code: string;
  name: string;
  level: string;
  status: 'potential' | 'following' | 'signed' | 'lost';
  ownerId: string;
  deptId: string | null;
  phone: string | null;
  email: string | null;
  remark: string | null;
  createdAt: string;
  updatedAt: string | null;
}

interface PageResult<T> {
  data: T[];
  page: { page: number; size: number; total: number; hasNext: boolean };
}

const { t } = useI18n();
const queryClient = useQueryClient();
const searchText = ref('');
const keyword = ref('');
const page = ref(1);
const size = ref(20);

const query = useQuery({
  queryKey: computed(() => ['crm-customers', page.value, size.value, keyword.value] as const),
  queryFn: () => {
    const params = new URLSearchParams({ page: String(page.value), size: String(size.value) });
    if (keyword.value.trim()) params.set('keyword', keyword.value.trim());
    return http.get<PageResult<CustomerVo>>(`/api/crm/customers?${params.toString()}`);
  },
  placeholderData: keepPreviousData,
});
const customers = computed(() => query.data.value?.data ?? []);
const total = computed(() => query.data.value?.page.total ?? 0);
const loading = computed(() => query.isFetching.value);

watch(query.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('crm.loadFailed'));
});

const columns = computed(() => [
  { title: t('crm.colCode'), dataIndex: 'code', key: 'code', width: 140 },
  { title: t('crm.customerName'), dataIndex: 'name', key: 'name' },
  { title: t('crm.colLevel'), dataIndex: 'level', key: 'level', width: 110 },
  { title: t('crm.colStatus'), dataIndex: 'status', key: 'status', width: 110 },
  { title: t('crm.colPhone'), dataIndex: 'phone', key: 'phone', width: 140 },
  { title: t('crm.colCreated'), dataIndex: 'createdAt', key: 'createdAt', width: 190 },
  { title: t('crm.colActions'), key: 'actions', width: 90 },
]);

const levelOptions = computed(() => [
  { label: t('crm.levelNormal'), value: 'normal' },
  { label: t('crm.levelImportant'), value: 'important' },
  { label: t('crm.levelStrategic'), value: 'strategic' },
]);

function levelText(level: string): string {
  return levelOptions.value.find((o: { value: string; label: string }) => o.value === level)?.label ?? level;
}

function statusText(status: CustomerVo['status']): string {
  return (
    { potential: t('crm.statusPotential'), following: t('crm.statusFollowing'), signed: t('crm.statusSigned'), lost: t('crm.statusLost') }[status] ?? status
  );
}

function statusColor(status: CustomerVo['status']): string {
  return { potential: 'default', following: 'blue', signed: 'green', lost: 'red' }[status] ?? 'default';
}

function reload(): void {
  void query.refetch();
}

function onSearch(): void {
  page.value = 1;
  keyword.value = searchText.value;
  void queryClient.invalidateQueries({ queryKey: ['crm-customers'] });
}

function onTableChange(pagination: TablePaginationConfig): void {
  page.value = pagination.current ?? 1;
  size.value = pagination.pageSize ?? 20;
}

// ---- 导入 / 导出 ----
interface ImportErrorRow {
  row: number;
  message: string;
}

interface ImportResult {
  total: number;
  success: number;
  failed: number;
  errors: ImportErrorRow[];
}

interface UploadRequestOption {
  file: File;
  onSuccess?: (body: unknown) => void;
  onError?: (err: Error) => void;
}

const importing = ref(false);
const importOpen = ref(false);
const importResult = ref<ImportResult | null>(null);

/** 导出：浏览器直接下载（后端返回 xlsx 流）。 */
function onExport(): void {
  window.open('/api/crm/customers/export', '_blank');
}

/** 导入：multipart 上传，返回逐行结果。 */
async function onImport(options: UploadRequestOption): Promise<void> {
  const form = new FormData();
  form.append('file', options.file);

  importing.value = true;
  try {
    const response = await fetch('/api/crm/customers/import', {
      method: 'POST',
      body: form,
      credentials: 'include',
    });
    const payload = await response.json().catch(() => ({}));
    if (!response.ok) {
      throw new ApiError(payload.code ?? 'import.failed', payload.detail ?? t('crm.importFailed'), response.status);
    }
    importResult.value = payload.data as ImportResult;
    importOpen.value = true;
    options.onSuccess?.(payload);
    void queryClient.invalidateQueries({ queryKey: ['crm-customers'] });
  } catch (error) {
    const err = error instanceof ApiError ? error : new ApiError('import.failed', t('crm.importFailed'), 0);
    message.error(err.message);
    options.onError?.(err as unknown as Error);
  } finally {
    importing.value = false;
  }
}

// ---- 新建 ----
const createOpen = ref(false);

const deleteMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/crm/customers/${id}`),
  onSuccess: () => {
    message.success(t('crm.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['crm-customers'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('crm.deleteFailed'));
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
