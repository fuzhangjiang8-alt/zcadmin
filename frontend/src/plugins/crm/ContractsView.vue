<template>
  <div class="page">
    <a-card :bordered="false" :title="t('crm.contractsTitle')">
      <template #extra>
        <a-space>
          <a-upload :before-upload="onImport" :show-upload-list="false" accept=".xlsx">
            <a-button v-perm="PERMS.CONTRACT_ADD" :loading="importing">{{ t('crm.importBtn') }}</a-button>
          </a-upload>
          <a-button v-perm="PERMS.CONTRACT_LIST" :loading="exporting" @click="exportNow">{{ t('crm.exportBtn') }}</a-button>
          <a-input-search
            v-model:value="searchText"
            :placeholder="t('crm.searchContract')"
            allow-clear
            style="width: 260px"
            @search="onSearch"
          />
          <a-button v-perm="PERMS.CONTRACT_ADD" type="primary" @click="openCreate">
            {{ t('crm.newContract') }}
          </a-button>
        </a-space>
      </template>

      <a-table
        :columns="columns"
        :data-source="rows"
        :loading="loading"
        row-key="id"
        :pagination="{
          current: page,
          pageSize: size,
          total,
          showSizeChanger: true,
          showTotal: (total: number) => t('crm.totalContracts', { n: total }),
        }"
        @change="onTableChange"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'contractNo'">
            <router-link :to="{ name: 'crm-contract-detail', params: { id: record.id } }">
              {{ record.contractNo }}
            </router-link>
          </template>
          <template v-else-if="column.key === 'customer'">{{ customerName(record.customerId) }}</template>
          <template v-else-if="column.key === 'amount'">{{ formatAmount(record.amountCents) }}</template>
          <template v-else-if="column.key === 'paid'">
            <span :class="{ 'text-green': record.paidCents >= record.amountCents }">
              {{ formatAmount(record.paidCents) }}
            </span>
          </template>
          <template v-else-if="column.key === 'status'">
            <a-tag :color="statusColor(record.status)">{{ statusText(record.status) }}</a-tag>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-space>
              <a-button v-perm="PERMS.CONTRACT_EDIT" type="link" size="small" @click="openEdit(record)">
                {{ t('crm.edit') }}
              </a-button>
              <a-popconfirm :title="t('crm.deleteConfirmContract')" @confirm="onDelete(record)">
                <a-button v-perm="PERMS.CONTRACT_DELETE" type="link" size="small" danger>
                  {{ t('crm.delete') }}
                </a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>

    <ContractEditModal v-model:open="editOpen" :contract="editing" @saved="reload" />
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../../permissions';
// =============================================================================
// CRM 合同
//
// 合同金额用分存储；状态：草稿/生效/完成/作废。
// 编辑时不允许更换客户，避免历史对账错乱。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { TablePaginationConfig } from 'ant-design-vue';
import { ApiError, http } from '../../api/client';
import { useExport } from './useExport';
import { useImport } from './useImport';
import ContractEditModal from './ContractEditModal.vue';

interface ContractVo {
  id: string;
  customerId: string;
  contractNo: string;
  name: string;
  amountCents: number;
  paidCents: number;
  status: string;
}

interface CustomerVo {
  id: string;
  name: string;
}

interface PageResult<T> {
  data: T[];
  page: { page: number; size: number; total: number; hasNext: boolean };
}

const statusOptions = computed(() => [
  { label: t('crm.statusDraft'), value: 'draft' },
  { label: t('crm.statusActive'), value: 'active' },
  { label: t('crm.statusCompleted'), value: 'completed' },
  { label: t('crm.statusVoid'), value: 'void' },
]);

const { t } = useI18n();
const queryClient = useQueryClient();
const searchText = ref('');
const keyword = ref('');
const page = ref(1);
const size = ref(20);

const query = useQuery({
  queryKey: computed(() => ['crm-contracts', page.value, size.value, keyword.value] as const),
  queryFn: () => {
    const params = new URLSearchParams({ page: String(page.value), size: String(size.value) });
    if (keyword.value.trim()) params.set('keyword', keyword.value.trim());
    return http.get<PageResult<ContractVo>>(`/api/crm/contracts?${params.toString()}`);
  },
  placeholderData: keepPreviousData,
});
const rows = computed(() => query.data.value?.data ?? []);
const total = computed(() => query.data.value?.page.total ?? 0);
const loading = computed(() => query.isFetching.value);

watch(query.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('crm.loadFailed'));
});

const customersQuery = useQuery({
  queryKey: ['crm-customers-options'],
  queryFn: () => http.get<PageResult<CustomerVo>>('/api/crm/customers?page=1&size=200'),
  staleTime: 5 * 60_000,
});
const customers = computed(() => customersQuery.data.value?.data ?? []);

const { exporting, exportNow } = useExport('crm.contracts');
const { importing, importFile } = useImport('crm.contracts');

function reload(): void {
  void query.refetch();
}

function onSearch(): void {
  page.value = 1;
  keyword.value = searchText.value;
  void queryClient.invalidateQueries({ queryKey: ['crm-contracts'] });
}

function onImport(file: File): boolean {
  void importFile(file).then((ok) => {
    if (ok) void queryClient.invalidateQueries({ queryKey: ['crm-contracts'] });
  });
  return false;
}

const columns = computed(() => [
  { title: t('crm.colContractNo'), dataIndex: 'contractNo', key: 'contractNo', width: 180 },
  { title: t('crm.colContractName'), dataIndex: 'name', key: 'name', width: 220 },
  { title: t('crm.colCustomer'), dataIndex: 'customerId', key: 'customer', width: 180 },
  { title: t('crm.colAmount'), dataIndex: 'amountCents', key: 'amount', width: 140 },
  { title: t('crm.colPaid'), dataIndex: 'paidCents', key: 'paid', width: 140 },
  { title: t('crm.colStatus'), dataIndex: 'status', key: 'status', width: 100 },
  { title: t('crm.colActions'), key: 'actions', width: 150 },
]);

const statusText = (status: string): string =>
  statusOptions.value.find((s) => s.value === status)?.label ?? status;
const statusColor = (status: string): string =>
  ({ draft: 'default', active: 'blue', completed: 'green', void: 'red' })[status] ?? 'default';

function customerName(id: string): string {
  return customers.value.find((c) => c.id === id)?.name ?? id.slice(0, 8);
}

function formatAmount(cents: number): string {
  return `¥${(cents / 100).toLocaleString('zh-CN', { minimumFractionDigits: 2 })}`;
}

function onTableChange(pagination: TablePaginationConfig): void {
  page.value = pagination.current ?? 1;
  size.value = pagination.pageSize ?? 20;
}

const editOpen = ref(false);
const editing = ref<ContractVo | null>(null);

function openCreate(): void {
  editing.value = null;
  editOpen.value = true;
}

function openEdit(record: ContractVo): void {
  editing.value = record;
  editOpen.value = true;
}

const deleteMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/crm/contracts/${id}`),
  onSuccess: () => {
    message.success(t('crm.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['crm-contracts'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('crm.deleteFailed'));
  },
});

function onDelete(record: ContractVo): void {
  deleteMutation.mutate(record.id);
}

</script>

<style scoped>
.page {
  padding: 0;
}

.text-green {
  color: #16a34a;
}
</style>
