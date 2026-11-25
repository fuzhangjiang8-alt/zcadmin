<template>
  <div class="page">
    <a-card :bordered="false" :title="t('crm.dashboardTitle')">
      <template #extra>
        <a-button :loading="loading" @click="reload">{{ t('crm.refresh') }}</a-button>
      </template>

      <a-spin :spinning="loading">
        <a-row :gutter="16" class="cards">
          <a-col :span="6">
            <a-card size="small" :bordered="false" class="stat">
              <div class="stat-label">{{ t('crm.contractAmount') }}</div>
              <div class="stat-value">{{ formatAmount(summary?.contractAmountCents ?? 0) }}</div>
              <div class="stat-sub">{{ t('crm.contracts', { n: summary?.contractCount ?? 0 }) }}</div>
            </a-card>
          </a-col>
          <a-col :span="6">
            <a-card size="small" :bordered="false" class="stat">
              <div class="stat-label">{{ t('crm.paid') }}</div>
              <div class="stat-value green">{{ formatAmount(summary?.paidCents ?? 0) }}</div>
              <div class="stat-sub">{{ t('crm.remaining', { amount: formatAmount(summary?.remainingCents ?? 0) }) }}</div>
            </a-card>
          </a-col>
          <a-col :span="6">
            <a-card size="small" :bordered="false" class="stat">
              <div class="stat-label">{{ t('crm.payRate') }}</div>
              <div class="stat-value">{{ collectionRate }}</div>
              <div class="stat-sub">{{ t('crm.paidOverContract') }}</div>
            </a-card>
          </a-col>
          <a-col :span="6">
            <a-card size="small" :bordered="false" class="stat">
              <div class="stat-label">{{ t('crm.customersOpportunities') }}</div>
              <div class="stat-value">
                {{ summary?.customerCount ?? 0 }} / {{ summary?.opportunityCount ?? 0 }}
              </div>
              <div class="stat-sub">{{ t('crm.customersOpps', { c: summary?.customerCount ?? 0, o: summary?.opportunityCount ?? 0 }) }}</div>
            </a-card>
          </a-col>
        </a-row>

        <a-divider orientation="left">{{ t('crm.trend6m') }}</a-divider>

        <div v-for="m in summary?.months ?? []" :key="m.month" class="month-row">
          <div class="month-label">{{ m.month }}</div>
          <div class="bars">
            <div class="bar-line">
              <span class="bar-name">{{ t('crm.contract') }}</span>
              <div class="bar">
                <div
                  class="bar-fill contract"
                  :style="{ width: `${barWidth(m.contractCents)}%` }"
                />
              </div>
              <span class="bar-value">{{ formatAmount(m.contractCents) }}</span>
            </div>
            <div class="bar-line">
              <span class="bar-name">{{ t('crm.payment') }}</span>
              <div class="bar">
                <div
                  class="bar-fill payment"
                  :style="{ width: `${barWidth(m.paymentCents)}%` }"
                />
              </div>
              <span class="bar-value">{{ formatAmount(m.paymentCents) }}</span>
            </div>
          </div>
        </div>
        <DashboardTables
          :top-customers="topCustomers"
          :stage-durations="stageDurations"
          :conversions="conversions"
          :stuck="stuck"
        />
      </a-spin>
    </a-card>
  </div>
</template>

<script setup lang="ts">
// =============================================================================
// CRM 经营看板
//
// 合同额、回款额、回款率与近 6 个月趋势；所有数字都按数据权限过滤。
// =============================================================================

import { computed, watch } from 'vue';
import { useQuery } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';
import { formatAmount } from './format';
import DashboardTables from './DashboardTables.vue';

interface MonthStat {
  month: string;
  contractCents: number;
  paymentCents: number;
}

interface DashboardSummary {
  contractAmountCents: number;
  paidCents: number;
  remainingCents: number;
  contractCount: number;
  customerCount: number;
  opportunityCount: number;
  months: MonthStat[];
}

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
const query = useQuery({
  queryKey: ['crm-dashboard'],
  queryFn: async () => {
    const [summary, topCustomers, stageDurations, conversions, stuck] = await Promise.all([
      http.get<DashboardSummary>('/api/crm/dashboard/summary'),
      http.get<TopCustomer[]>('/api/crm/dashboard/top-customers?limit=5'),
      http.get<StageDuration[]>('/api/crm/opportunities/stage-duration'),
      http.get<StageConversion[]>('/api/crm/opportunities/stage-conversion'),
      http.get<StuckOpportunity[]>('/api/crm/opportunities/stuck?days=14'),
    ]);
    return { summary, topCustomers, stageDurations, conversions, stuck };
  },
});
const summary = computed(() => query.data.value?.summary ?? null);
const topCustomers = computed(() => query.data.value?.topCustomers ?? []);
const stageDurations = computed(() => query.data.value?.stageDurations ?? []);
const conversions = computed(() => query.data.value?.conversions ?? []);
const stuck = computed(() => query.data.value?.stuck ?? []);
const loading = computed(() => query.isFetching.value);
watch(query.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('crm.loadFailed'));
});

const maxMonth = computed(() => {
  const months = summary.value?.months ?? [];
  return Math.max(1, ...months.flatMap((m) => [m.contractCents, m.paymentCents]));
});

const collectionRate = computed(() => {
  const s = summary.value;
  if (!s || s.contractAmountCents === 0) return '—';
  return `${Math.round((s.paidCents / s.contractAmountCents) * 100)}%`;
});

function barWidth(value: number): number {
  return Math.max(1, Math.round((value / maxMonth.value) * 100));
}

function reload(): void {
  void query.refetch();
}
</script>

<style scoped>
.page {
  padding: 0;
}

.cards {
  margin-bottom: 8px;
}

.stat {
  background: #f8fafc;
}

.stat-label {
  color: #64748b;
  font-size: 12px;
}

.stat-value {
  font-size: 22px;
  font-weight: 600;
  margin-top: 4px;
}

.stat-value.green {
  color: #16a34a;
}

.stat-sub {
  color: #94a3b8;
  font-size: 12px;
  margin-top: 2px;
}

.month-row {
  display: flex;
  gap: 16px;
  padding: 10px 0;
  border-bottom: 1px solid #f1f5f9;
}

.month-label {
  width: 72px;
  color: #475569;
  font-size: 13px;
  padding-top: 4px;
}

.bars {
  flex: 1;
}

.bar-line {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
}

.bar-name {
  width: 32px;
  color: #94a3b8;
  font-size: 12px;
}

.bar {
  flex: 1;
  height: 8px;
  background: #f1f5f9;
  border-radius: 4px;
  overflow: hidden;
}

.bar-fill {
  height: 100%;
  border-radius: 4px;
}

.bar-fill.contract {
  background: #2563eb;
}

.bar-fill.payment {
  background: #16a34a;
}

.bar-value {
  width: 130px;
  text-align: right;
  font-size: 12px;
  color: #475569;
}
</style>
