<template>
  <div class="page">
    <a-card :bordered="false">
      <template #title>
        <a-space>
          <a-button type="link" size="small" @click="router.back()">{{ t('crm.back') }}</a-button>
          <span>{{ contract?.contractNo ?? t('crm.contractDetail') }}</span>
        </a-space>
      </template>

      <a-spin :spinning="loading">
        <a-descriptions v-if="contract" :column="3" size="small" bordered>
          <a-descriptions-item :label="t('crm.colContractNo')">{{ contract.contractNo }}</a-descriptions-item>
          <a-descriptions-item :label="t('crm.colContractName')">{{ contract.name }}</a-descriptions-item>
          <a-descriptions-item :label="t('crm.status')">
            <a-tag :color="statusColor(contract.status)">{{ statusText(contract.status) }}</a-tag>
          </a-descriptions-item>
          <a-descriptions-item :label="t('crm.contractAmountLabel')">{{ formatAmount(contract.amountCents) }}</a-descriptions-item>
          <a-descriptions-item :label="t('crm.paidLabel')">
            <span class="green">{{ formatAmount(summary?.paidCents ?? contract.paidCents) }}</span>
          </a-descriptions-item>
          <a-descriptions-item :label="t('crm.unpaid')">
            {{ formatAmount(summary?.remainingCents ?? contract.amountCents - contract.paidCents) }}
          </a-descriptions-item>
        </a-descriptions>

        <div v-if="contract" class="progress-wrap">
          <a-progress
            :percent="progressPercent"
            :status="progressPercent >= 100 ? 'success' : 'active'"
          />
        </div>

        <a-divider orientation="left">{{ t('crm.paymentRecords', { n: summary?.items.length ?? 0 }) }}</a-divider>
        <a-table
          size="small"
          :columns="columns"
          :data-source="summary?.items ?? []"
          row-key="id"
          :pagination="{
            current: page,
            pageSize: size,
            total: summary?.total ?? 0,
            showSizeChanger: true,
            showTotal: (total: number) => t('crm.totalPayments', { n: total }),
          }"
          @change="onTableChange"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'amount'">{{ formatAmount(record.amountCents) }}</template>
            <template v-else-if="column.key === 'paidAt'">{{ record.paidAt ?? '-' }}</template>
            <template v-else-if="column.key === 'method'">{{ methodText(record.method) }}</template>
          </template>
        </a-table>
        <div v-if="(summary?.items.length ?? 0) === 0" class="empty">{{ t('crm.noPayments') }}</div>
      </a-spin>
    </a-card>
  </div>
</template>

<script setup lang="ts">
// =============================================================================
// 合同详情：合同信息 + 回款进度 + 最近 200 条回款
//
// 汇总金额来自 contract.paid_cents（预聚合），明细来自 /contracts/{id}/payments。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { keepPreviousData, useQuery } from '@tanstack/vue-query';
import { useRoute, useRouter } from 'vue-router';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';

interface ContractVo {
  id: string;
  contractNo: string;
  name: string;
  amountCents: number;
  paidCents: number;
  status: string;
}

interface PaymentVo {
  id: string;
  paymentNo: string;
  amountCents: number;
  paidAt: string | null;
  method: string | null;
}

interface ContractPayments {
  contractAmountCents: number;
  paidCents: number;
  remainingCents: number;
  items: PaymentVo[];
  total: number;
  page: number;
  size: number;
}

const route = useRoute();
const router = useRouter();
const id = String(route.params.id);

const { t } = useI18n();
const page = ref(1);
const size = ref(50);

const contractQuery = useQuery({
  queryKey: ['crm-contract', id],
  queryFn: () => http.get<ContractVo>(`/api/crm/contracts/${id}`),
});
const paymentsQuery = useQuery({
  queryKey: computed(() => ['crm-contract-payments', id, page.value, size.value] as const),
  queryFn: () => {
    const params = new URLSearchParams({ page: String(page.value), size: String(size.value) });
    return http.get<ContractPayments>(`/api/crm/contracts/${id}/payments?${params.toString()}`);
  },
  placeholderData: keepPreviousData,
});
const contract = computed(() => contractQuery.data.value ?? null);
const summary = computed(() => paymentsQuery.data.value ?? null);
const loading = computed(() => contractQuery.isFetching.value || paymentsQuery.isFetching.value);
watch([contractQuery.error, paymentsQuery.error], ([contractError, paymentsError]) => {
  const error = contractError ?? paymentsError;
  if (error) message.error(error instanceof ApiError ? error.message : t('crm.loadFailed'));
});

const columns = computed(() => [
  { title: t('crm.colPaymentNo'), dataIndex: 'paymentNo', key: 'paymentNo', width: 180 },
  { title: t('crm.colAmount'), dataIndex: 'amountCents', key: 'amount', width: 140 },
  { title: t('crm.colPaidAt'), dataIndex: 'paidAt', key: 'paidAt', width: 190 },
  { title: t('crm.colMethod'), dataIndex: 'method', key: 'method', width: 130 },
]);

const progressPercent = computed(() => {
  const s = summary.value;
  if (!s || s.contractAmountCents === 0) return 0;
  return Math.min(100, Math.round((s.paidCents / s.contractAmountCents) * 100));
});

const statusText = (status: string): string =>
  ({ draft: t('crm.statusDraft'), active: t('crm.statusActive'), completed: t('crm.statusCompleted'), void: t('crm.statusVoid') })[status] ?? status;
const statusColor = (status: string): string =>
  ({ draft: 'default', active: 'blue', completed: 'green', void: 'red' })[status] ?? 'default';
const methodText = (method: string | null): string =>
  method
    ? ({ transfer: t('crm.methodTransfer'), acceptance: t('crm.methodAcceptance'), cash: t('crm.methodCash'), other: t('crm.methodOther') })[method] ?? method
    : '-';

function formatAmount(cents: number): string {
  return `¥${(cents / 100).toLocaleString('zh-CN', { minimumFractionDigits: 2 })}`;
}

function onTableChange(pagination: { current?: number; pageSize?: number }): void {
  page.value = pagination.current ?? 1;
  size.value = pagination.pageSize ?? 50;
}


</script>

<style scoped>
.page {
  padding: 0;
}

.progress-wrap {
  margin-top: 16px;
  max-width: 480px;
}

.green {
  color: #16a34a;
  font-weight: 600;
}

.empty {
  color: #94a3b8;
  padding: 12px 0;
}
</style>
