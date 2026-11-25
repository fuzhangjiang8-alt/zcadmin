<template>
  <a-divider orientation="left">{{ t('crm.topCustomers') }}</a-divider>
  <a-table size="small" :columns="topColumns" :data-source="topCustomers" row-key="customerId" :pagination="false">
    <template #bodyCell="{ column, record }">
      <template v-if="column.key === 'name'">
        <router-link :to="{ name: 'crm-customer-detail', params: { id: record.customerId } }">
          {{ record.name }}
        </router-link>
      </template>
      <template v-else-if="column.key === 'contract'">{{ formatAmount(record.contractCents) }}</template>
      <template v-else-if="column.key === 'paid'">
        <span class="green">{{ formatAmount(record.paidCents) }}</span>
      </template>
    </template>
  </a-table>

  <a-divider orientation="left">{{ t('crm.stageAvgStay') }}</a-divider>
  <a-table size="small" :columns="stageColumns" :data-source="stageDurations" row-key="stage" :pagination="false">
    <template #bodyCell="{ column, record }">
      <template v-if="column.key === 'stage'">{{ stageLabel(record.stage) }}</template>
      <template v-else-if="column.key === 'avg'">{{ record.avgDays.toFixed(1) }}</template>
    </template>
  </a-table>

  <a-divider orientation="left">{{ t('crm.stageConversion') }}</a-divider>
  <a-table size="small" :columns="conversionColumns" :data-source="conversions" row-key="fromStage" :pagination="false">
    <template #bodyCell="{ column, record }">
      <template v-if="column.key === 'from'">{{ stageLabel(record.fromStage) }}</template>
      <template v-else-if="column.key === 'to'">{{ stageLabel(record.toStage) }}</template>
      <template v-else-if="column.key === 'percent'">{{ record.percent.toFixed(1) }}%</template>
    </template>
  </a-table>

  <a-divider orientation="left">{{ t('crm.stageOverdue') }}</a-divider>
  <a-table size="small" :columns="stuckColumns" :data-source="stuck" row-key="opportunityId" :pagination="false">
    <template #bodyCell="{ column, record }">
      <template v-if="column.key === 'stage'">{{ stageLabel(record.stage) }}</template>
      <template v-else-if="column.key === 'days'">
        <a-tag :color="record.daysInStage >= 30 ? 'red' : 'orange'">
          {{ t('crm.days', { n: record.daysInStage.toFixed(1) }) }}
        </a-tag>
      </template>
      <template v-else-if="column.key === 'amount'">{{ formatAmount(record.amountCents ?? 0) }}</template>
    </template>
  </a-table>
</template>

<script setup lang="ts">
// =============================================================================
// 经营看板：客户/阶段/转化/超时四张表
//
// 纯展示组件，数据由父页面一次性拉取后传入；不发起请求。
// =============================================================================

import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { formatAmount, stageLabel } from './format';

interface StuckOpportunity {
  opportunityId: string;
  name: string;
  customerName: string;
  stage: string;
  amountCents: number | null;
  daysInStage: number;
}

interface StageConversion {
  fromStage: string;
  toStage: string;
  count: number;
  percent: number;
}

interface StageDuration {
  stage: string;
  avgDays: number;
  changes: number;
}

interface TopCustomer {
  customerId: string;
  name: string;
  contractCount: number;
  contractCents: number;
  paidCents: number;
}

const { t } = useI18n();
defineProps<{
  topCustomers: TopCustomer[];
  stageDurations: StageDuration[];
  conversions: StageConversion[];
  stuck: StuckOpportunity[];
}>();

const stuckColumns = computed(() => [
  { title: t('crm.colOpportunity'), dataIndex: 'name', key: 'name' },
  { title: t('crm.colCustomer'), dataIndex: 'customerName', key: 'customerName' },
  { title: t('crm.colStage'), dataIndex: 'stage', key: 'stage', width: 110 },
  { title: t('crm.colStay'), dataIndex: 'daysInStage', key: 'days', width: 110 },
  { title: t('crm.colAmount'), dataIndex: 'amountCents', key: 'amount', width: 130 },
]);
const conversionColumns = computed(() => [
  { title: t('crm.colFrom'), dataIndex: 'fromStage', key: 'from', width: 140 },
  { title: t('crm.colTo'), dataIndex: 'toStage', key: 'to', width: 140 },
  { title: t('crm.colCount'), dataIndex: 'count', key: 'count', width: 100 },
  { title: t('crm.colPercent'), dataIndex: 'percent', key: 'percent', width: 110 },
]);
const stageColumns = computed(() => [
  { title: t('crm.colStage'), dataIndex: 'stage', key: 'stage' },
  { title: t('crm.colAvgStay'), dataIndex: 'avgDays', key: 'avg', width: 140 },
  { title: t('crm.colChanges'), dataIndex: 'changes', key: 'changes', width: 120 },
]);
const topColumns = computed(() => [
  { title: t('crm.colCustomer'), dataIndex: 'name', key: 'name' },
  { title: t('crm.colContracts'), dataIndex: 'contractCount', key: 'contractCount', width: 90 },
  { title: t('crm.colContractAmount'), dataIndex: 'contractCents', key: 'contract', width: 150 },
  { title: t('crm.colPaid'), dataIndex: 'paidCents', key: 'paid', width: 150 },
]);
</script>
