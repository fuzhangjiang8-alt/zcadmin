<template>
  <div class="page">
    <a-card :bordered="false" :title="t('crm.pipelineTitle')">
      <template #extra>
        <a-button :loading="loading" @click="reload">{{ t('crm.refresh') }}</a-button>
      </template>

      <a-row :gutter="16" class="totals">
        <a-col :span="8">
          <a-card size="small" :bordered="false" class="stat">
            <div class="stat-label">{{ t('crm.totalOpps') }}</div>
            <div class="stat-value">{{ totalCount }}</div>
          </a-card>
        </a-col>
        <a-col :span="8">
          <a-card size="small" :bordered="false" class="stat">
            <div class="stat-label">{{ t('crm.totalAmount') }}</div>
            <div class="stat-value">{{ formatAmount(totalAmount) }}</div>
          </a-card>
        </a-col>
        <a-col :span="8">
          <a-card size="small" :bordered="false" class="stat">
            <div class="stat-label">{{ t('crm.winRate') }}</div>
            <div class="stat-value">{{ winRate }}</div>
          </a-card>
        </a-col>
      </a-row>

      <a-spin :spinning="loading">
        <div v-for="row in rows" :key="row.stage" class="funnel-row">
          <div class="funnel-label">
            <a-tag :color="stageColor(row.stage)">{{ stageText(row.stage) }}</a-tag>
            <span class="count">{{ t('crm.countUnit', { n: row.count }) }}</span>
            <span class="amount">{{ formatAmount(row.amountCents) }}</span>
          </div>
          <div class="funnel-bar">
            <div
              class="funnel-fill"
              :style="{ width: `${barWidth(row.amountCents)}%`, background: stageColor(row.stage) }"
            />
          </div>
        </div>
      </a-spin>
    </a-card>
  </div>
</template>

<script setup lang="ts">
// =============================================================================
// 商机漏斗
//
// 按阶段汇总数量与金额（后端已按数据权限过滤）。
// 柱宽以最大阶段金额为基准，避免金额差异大时全部变成细线。
// =============================================================================

import { computed, watch } from 'vue';
import { useQuery } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';

interface StageStat {
  stage: string;
  count: number;
  amountCents: number;
}

const STAGES = computed(() => [
  { value: 'initial', label: t('crm.stageInitial'), color: '#94a3b8' },
  { value: 'proposal', label: t('crm.stageProposal'), color: '#2563eb' },
  { value: 'negotiation', label: t('crm.stageNegotiation'), color: '#d97706' },
  { value: 'won', label: t('crm.stageWon'), color: '#16a34a' },
  { value: 'lost', label: t('crm.stageLost'), color: '#dc2626' },
]);

const { t } = useI18n();
const query = useQuery({
  queryKey: ['crm-pipeline'],
  queryFn: () => http.get<StageStat[]>('/api/crm/opportunities/stats'),
});
const stats = computed(() => query.data.value ?? []);
const loading = computed(() => query.isFetching.value);
watch(query.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('crm.loadFailed'));
});

const rows = computed(() =>
  STAGES.value.map((s) => {
    const hit = stats.value.find((x) => x.stage === s.value);
    return { stage: s.value, count: hit?.count ?? 0, amountCents: hit?.amountCents ?? 0 };
  }),
);

const totalCount = computed(() => rows.value.reduce((sum, r) => sum + r.count, 0));
const totalAmount = computed(() => rows.value.reduce((sum, r) => sum + r.amountCents, 0));
const maxAmount = computed(() => Math.max(1, ...rows.value.map((r) => r.amountCents)));

const winRate = computed(() => {
  const won = rows.value.find((r) => r.stage === 'won')?.count ?? 0;
  const lost = rows.value.find((r) => r.stage === 'lost')?.count ?? 0;
  const closed = won + lost;
  return closed === 0 ? t('crm.noData') : `${Math.round((won / closed) * 100)}%`;
});

const stageText = (stage: string): string =>
  STAGES.value.find((s) => s.value === stage)?.label ?? stage;
const stageColor = (stage: string): string =>
  STAGES.value.find((s) => s.value === stage)?.color ?? '#94a3b8';

function barWidth(amount: number): number {
  return Math.max(2, Math.round((amount / maxAmount.value) * 100));
}

function formatAmount(cents: number): string {
  return `¥${(cents / 100).toLocaleString('zh-CN', { minimumFractionDigits: 2 })}`;
}

function reload(): void {
  void query.refetch();
}
</script>

<style scoped>
.page {
  padding: 0;
}

.totals {
  margin-bottom: 24px;
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

.funnel-row {
  margin-bottom: 16px;
}

.funnel-label {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 6px;
}

.count {
  color: #64748b;
  font-size: 12px;
}

.amount {
  margin-left: auto;
  font-weight: 600;
  font-size: 13px;
}

.funnel-bar {
  height: 10px;
  background: #f1f5f9;
  border-radius: 6px;
  overflow: hidden;
}

.funnel-fill {
  height: 100%;
  border-radius: 6px;
  transition: width 0.3s ease;
}
</style>
