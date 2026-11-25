<template>
  <div class="page">
    <a-card :bordered="false" :title="t('crm.paymentsTitle')">
      <template #extra>
        <a-space>
          <a-upload :before-upload="onImport" :show-upload-list="false" accept=".xlsx">
            <a-button v-perm="PERMS.PAYMENT_ADD" :loading="importing">{{ t('crm.importBtn') }}</a-button>
          </a-upload>
          <a-button v-perm="PERMS.PAYMENT_LIST" :loading="exporting" @click="exportNow">{{ t('crm.exportBtn') }}</a-button>
          <a-range-picker
            v-model:value="paidRange"
            value-format="YYYY-MM-DD"
            style="width: 240px"
            @change="applyFilter"
          />
          <a-input-search
            v-model:value="searchText"
            :placeholder="t('crm.searchPaymentNo')"
            allow-clear
            style="width: 240px"
            @search="applyFilter"
          />
          <a-button v-perm="PERMS.PAYMENT_ADD" type="primary" @click="openCreate">
            {{ t('crm.registerPayment') }}
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
          showTotal: (total: number) => t('crm.totalPayments', { n: total }),
        }"
        @change="onTableChange"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'contract'">{{ contractName(record.contractId) }}</template>
          <template v-else-if="column.key === 'amount'">{{ formatAmount(record.amountCents) }}</template>
          <template v-else-if="column.key === 'paidAt'">{{ record.paidAt ?? '-' }}</template>
          <template v-else-if="column.key === 'actions'">
            <a-space>
              <a-button v-perm="PERMS.PAYMENT_EDIT" type="link" size="small" @click="openEdit(record)">
                {{ t('crm.edit') }}
              </a-button>
              <a-popconfirm :title="t('crm.deleteConfirmPayment')" @confirm="onDelete(record)">
                <a-button v-perm="PERMS.PAYMENT_DELETE" type="link" size="small" danger>
                  {{ t('crm.delete') }}
                </a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>

    <PaymentEditModal v-model:open="editOpen" :payment="editing" @saved="reload" />
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../../permissions';
// =============================================================================
// CRM 回款
//
// 回款必须挂在合同下；金额用分存储。
// 合同的「已回/未回」由 /contracts/{id}/payments 汇总接口计算。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { TablePaginationConfig } from 'ant-design-vue';
import { ApiError, http } from '../../api/client';
import { useExport } from './useExport';
import { useImport } from './useImport';
import PaymentEditModal from './PaymentEditModal.vue';

interface PaymentVo {
  id: string;
  contractId: string;
  paymentNo: string;
  amountCents: number;
  paidAt: string | null;
  method: string | null;
}

interface ContractVo {
  id: string;
  contractNo: string;
  name: string;
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
const paidRange = ref<[string, string] | null>(null);

const query = useQuery({
  queryKey: computed(
    () => ['crm-payments', page.value, size.value, keyword.value, paidRange.value] as const,
  ),
  queryFn: () => {
    const params = new URLSearchParams({ page: String(page.value), size: String(size.value) });
    if (keyword.value.trim()) params.set('keyword', keyword.value.trim());
    if (paidRange.value && paidRange.value.length === 2) {
      params.set('paidFrom', paidRange.value[0]);
      params.set('paidTo', paidRange.value[1]);
    }
    return http.get<PageResult<PaymentVo>>(`/api/crm/payments?${params.toString()}`);
  },
  placeholderData: keepPreviousData,
});
const rows = computed(() => query.data.value?.data ?? []);
const total = computed(() => query.data.value?.page.total ?? 0);
const loading = computed(() => query.isFetching.value);

watch(query.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('crm.loadFailed'));
});

const contractsQuery = useQuery({
  queryKey: ['crm-contracts-options'],
  queryFn: () => http.get<PageResult<ContractVo>>('/api/crm/contracts?page=1&size=200'),
  staleTime: 5 * 60_000,
});
const contracts = computed(() => contractsQuery.data.value?.data ?? []);

const { exporting, exportNow } = useExport('crm.payments');
const { importing, importFile } = useImport('crm.payments');

function reload(): void {
  void query.refetch();
}

function applyFilter(): void {
  page.value = 1;
  keyword.value = searchText.value;
  void queryClient.invalidateQueries({ queryKey: ['crm-payments'] });
}

function onImport(file: File): boolean {
  void importFile(file).then((ok) => {
    if (ok) void queryClient.invalidateQueries({ queryKey: ['crm-payments'] });
  });
  return false;
}

const columns = computed(() => [
  { title: t('crm.colPaymentNo'), dataIndex: 'paymentNo', key: 'paymentNo', width: 180 },
  { title: t('crm.colContract'), dataIndex: 'contractId', key: 'contract', width: 220 },
  { title: t('crm.colAmount'), dataIndex: 'amountCents', key: 'amount', width: 140 },
  { title: t('crm.colPaidAt'), dataIndex: 'paidAt', key: 'paidAt', width: 190 },
  { title: t('crm.colMethod'), dataIndex: 'method', key: 'method', width: 120 },
  { title: t('crm.colActions'), key: 'actions', width: 150 },
]);

function contractName(id: string): string {
  const c = contracts.value.find((x) => x.id === id);
  return c ? `${c.contractNo} · ${c.name}` : id.slice(0, 8);
}

function formatAmount(cents: number): string {
  return `¥${(cents / 100).toLocaleString('zh-CN', { minimumFractionDigits: 2 })}`;
}

function onTableChange(pagination: TablePaginationConfig): void {
  page.value = pagination.current ?? 1;
  size.value = pagination.pageSize ?? 20;
}

const editOpen = ref(false);
const editing = ref<PaymentVo | null>(null);

function openCreate(): void {
  editing.value = null;
  editOpen.value = true;
}

function openEdit(record: PaymentVo): void {
  editing.value = record;
  editOpen.value = true;
}

const deleteMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/crm/payments/${id}`),
  onSuccess: () => {
    message.success(t('crm.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['crm-payments'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('crm.deleteFailed'));
  },
});

function onDelete(record: PaymentVo): void {
  deleteMutation.mutate(record.id);
}

</script>

<style scoped>
.page {
  padding: 0;
}
</style>
