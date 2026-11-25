<template>
  <a-modal
    :open="open"
    :title="t('tenants.detailTitle')"
    :footer="null"
    width="720px"
    @update:open="emit('update:open', $event)"
  >
    <a-spin :spinning="loading">
      <a-descriptions :column="2" size="small" bordered>
        <a-descriptions-item :label="t('tenants.dCode')">{{ detail?.code }}</a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dName')">{{ detail?.name }}</a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dStatus')">{{ detail?.status }}</a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dCreated')">{{ detail?.createdAt }}</a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dUsers')">
          {{ detail?.userCount }} / {{ detail?.maxUsers || t('tenants.unlimited') }}
        </a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dFiles')">{{ detail?.fileCount }}</a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dStorage')">{{ storageText(detail ?? undefined) }}</a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dSessions')">{{ detail?.sessionCount }}</a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dAudits')">{{ detail?.auditCount }}</a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dBill')">{{ billText }}</a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dCrmCustomer')">{{ detail?.customerCount }}</a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dCrmOpportunity')">{{ detail?.opportunityCount }}</a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dCrmContact')">{{ detail?.contactCount }}</a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dCrmContract')">{{ detail?.contractCount }}</a-descriptions-item>
        <a-descriptions-item :label="t('tenants.dCrmPayment')">{{ detail?.paymentCount }}</a-descriptions-item>
      </a-descriptions>
      <a-divider />
      <div class="section-title">{{ t('tenants.trendTitle') }}</div>
      <a-radio-group v-model:value="trendMetric" size="small" style="margin-bottom: 12px">
        <a-radio-button value="userCount">{{ t('tenants.trendUser') }}</a-radio-button>
        <a-radio-button value="storageMb">{{ t('tenants.trendStorage') }}</a-radio-button>
        <a-radio-button value="customerCount">{{ t('tenants.trendCustomer') }}</a-radio-button>
        <a-radio-button value="contractCount">{{ t('tenants.trendContract') }}</a-radio-button>
      </a-radio-group>
      <UsageTrendChart :title="trendTitle" :unit="trendUnit" :points="trendPoints" />
      <a-table
        :data-source="usagePoints"
        :columns="usageColumns"
        :pagination="false"
        size="small"
        row-key="snapshotDate"
      >
        <template #bodyCell="{ column, record }">
          <span v-if="column.key === 'storageUsed'">
            {{ (record.storageUsed / 1024 / 1024).toFixed(1) }} MB
          </span>
        </template>
      </a-table>
      <a-divider />
      <div class="section-title">{{ t('tenants.recentAudits') }}</div>
      <a-table
        :data-source="detail?.recentAudits ?? []"
        :columns="auditColumns"
        :pagination="false"
        size="small"
        row-key="createdAt"
      />
    </a-spin>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 租户详情弹窗
//
// 打开时并行拉取汇总、近 30 天用量、本月账单预估；三个接口互不依赖。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';
import UsageTrendChart from '../../components/UsageTrendChart.vue';
import { storageText } from './types';
import type { TenantBill, TenantSummary, TenantVo, UsagePoint } from './types';

const props = defineProps<{ open: boolean; tenant: TenantVo | null }>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

const { t } = useI18n();
const loading = ref(false);
const detail = ref<TenantSummary | null>(null);
const usagePoints = ref<UsagePoint[]>([]);
const bill = ref<TenantBill | null>(null);
const trendMetric = ref<'userCount' | 'storageMb' | 'customerCount' | 'contractCount'>('userCount');

const trendTitle = computed(
  () =>
    ({
      userCount: t('tenants.trendUsers'),
      storageMb: t('tenants.trendStorageUsed'),
      customerCount: t('tenants.dCrmCustomer'),
      contractCount: t('tenants.dCrmContract'),
    })[trendMetric.value],
);
const trendUnit = computed(() => (trendMetric.value === 'storageMb' ? 'MB' : ''));
const trendPoints = computed(() =>
  usagePoints.value.map((point) => ({
    date: point.snapshotDate,
    value:
      trendMetric.value === 'storageMb'
        ? Number((point.storageUsed / 1024 / 1024).toFixed(1))
        : point[trendMetric.value],
  })),
);
const billText = computed(() =>
  bill.value ? `${(bill.value.totalCents / 100).toFixed(2)} ${bill.value.currency}` : '-',
);

const usageColumns = computed(() => [
  { title: t('tenants.colDate'), dataIndex: 'snapshotDate', key: 'snapshotDate', width: 110 },
  { title: t('tenants.colUsers'), dataIndex: 'userCount', key: 'userCount', width: 80 },
  { title: t('tenants.dFiles'), dataIndex: 'fileCount', key: 'fileCount', width: 80 },
  { title: t('tenants.trendStorage'), dataIndex: 'storageUsed', key: 'storageUsed', width: 110 },
  { title: t('tenants.trendCustomer'), dataIndex: 'customerCount', key: 'customerCount', width: 80 },
  { title: t('tenants.trendContract'), dataIndex: 'contractCount', key: 'contractCount', width: 80 },
]);
const auditColumns = computed(() => [
  { title: t('tenants.colAction'), dataIndex: 'action', key: 'action' },
  { title: t('tenants.colTime'), dataIndex: 'createdAt', key: 'createdAt', width: 190 },
]);

async function load(): Promise<void> {
  if (!props.tenant) return;
  loading.value = true;
  try {
    const [summary, usage, currentBill] = await Promise.all([
      http.get<TenantSummary>(`/api/system/tenants/${props.tenant.id}/summary`),
      http.get<UsagePoint[]>(`/api/system/tenants/${props.tenant.id}/usage?days=30`),
      http.get<TenantBill>(`/api/system/tenants/${props.tenant.id}/bill`),
    ]);
    detail.value = summary;
    usagePoints.value = usage;
    bill.value = currentBill;
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('tenants.detailLoadFailed'));
  } finally {
    loading.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) void load();
  },
);
</script>
