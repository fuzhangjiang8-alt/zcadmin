<template>
  <div class="page">
    <a-card :bordered="false">
      <template #title>
        <a-space>
          <a-button type="link" size="small" @click="router.back()">{{ t('crm.back') }}</a-button>
          <span>{{ customer?.name ?? t('crm.customerDetail') }}</span>
        </a-space>
      </template>

      <a-spin :spinning="loading">
        <a-row v-if="customer" :gutter="16" class="kpi-row">
          <a-col :span="6">
            <a-card size="small" :bordered="false" class="kpi">
              <div class="kpi-label">{{ t('crm.contractTotal') }}</div>
              <div class="kpi-value">{{ formatAmount(contractTotal) }}</div>
              <div class="kpi-sub">{{ t('crm.contracts', { n: contracts.length }) }}</div>
            </a-card>
          </a-col>
          <a-col :span="6">
            <a-card size="small" :bordered="false" class="kpi">
              <div class="kpi-label">{{ t('crm.paidLabel') }}</div>
              <div class="kpi-value green">{{ formatAmount(paidTotal) }}</div>
            </a-card>
          </a-col>
          <a-col :span="6">
            <a-card size="small" :bordered="false" class="kpi">
              <div class="kpi-label">{{ t('crm.unpaid') }}</div>
              <div class="kpi-value">{{ formatAmount(contractTotal - paidTotal) }}</div>
            </a-card>
          </a-col>
          <a-col :span="6">
            <a-card size="small" :bordered="false" class="kpi">
              <div class="kpi-label">{{ t('crm.payRate') }}</div>
              <div class="kpi-value">{{ collectionRate }}</div>
            </a-card>
          </a-col>
        </a-row>

        <a-descriptions v-if="customer" :column="3" size="small" bordered>
          <a-descriptions-item :label="t('crm.colCode')">{{ customer.code }}</a-descriptions-item>
          <a-descriptions-item :label="t('crm.customerName')">{{ customer.name }}</a-descriptions-item>
          <a-descriptions-item :label="t('crm.colLevel')">{{ customer.level }}</a-descriptions-item>
          <a-descriptions-item :label="t('crm.colStatus')">{{ customer.status }}</a-descriptions-item>
          <a-descriptions-item :label="t('crm.colPhone')">{{ customer.phone ?? '-' }}</a-descriptions-item>
          <a-descriptions-item :label="t('crm.email')">{{ customer.email ?? '-' }}</a-descriptions-item>
          <a-descriptions-item :label="t('crm.colRemark')" :span="3">{{ customer.remark ?? '-' }}</a-descriptions-item>
        </a-descriptions>

        <a-divider orientation="left">{{ t('crm.contactsSection', { n: contacts.length }) }}</a-divider>
        <a-table
          size="small"
          :columns="contactColumns"
          :data-source="contacts"
          row-key="id"
          :pagination="false"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'isPrimary'">
              <a-tag v-if="record.isPrimary" color="blue">{{ t('crm.primary') }}</a-tag>
              <span v-else class="muted">—</span>
            </template>
          </template>
        </a-table>

        <a-divider orientation="left">{{ t('crm.opportunitiesSection', { n: opportunities.length }) }}</a-divider>
        <a-table
          size="small"
          :columns="opportunityColumns"
          :data-source="opportunities"
          row-key="id"
          :pagination="false"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'amount'">{{ formatAmount(record.amountCents) }}</template>
            <template v-else-if="column.key === 'stage'">
              <a-tag :color="stageColor(record.stage)">{{ stageText(record.stage) }}</a-tag>
            </template>
          </template>
        </a-table>

        <a-divider orientation="left">{{ t('crm.contractsSection', { n: contracts.length }) }}</a-divider>
        <div class="contract-summary">
          {{ t('crm.contractTotal') }} <strong>{{ formatAmount(contractTotal) }}</strong>
          <span class="divider">|</span>
          {{ t('crm.paidLabel') }} <strong class="green">{{ formatAmount(paidTotal) }}</strong>
        </div>
        <a-table
          size="small"
          :columns="contractColumns"
          :data-source="contracts"
          row-key="id"
          :pagination="false"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'amount'">{{ formatAmount(record.amountCents) }}</template>
            <template v-else-if="column.key === 'paid'">{{ formatAmount(record.paidCents) }}</template>
            <template v-else-if="column.key === 'status'">
              <a-tag :color="contractStatusColor(record.status)">
                {{ contractStatusText(record.status) }}
              </a-tag>
            </template>
          </template>
        </a-table>
      </a-spin>
    </a-card>
  </div>
</template>

<script setup lang="ts">
// =============================================================================
// 客户详情：客户信息 + 联系人 + 商机
//
// 数据权限：联系人/商机接口会先校验客户是否在当前用户的数据范围内，
// 越权访问返回 404（不是 403，避免暴露资源是否存在）。
// =============================================================================

import { computed, watch } from 'vue';
import { useQuery } from '@tanstack/vue-query';
import { useRoute, useRouter } from 'vue-router';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';

interface CustomerVo {
  id: string;
  code: string;
  name: string;
  level: string;
  status: string;
  phone: string | null;
  email: string | null;
  remark: string | null;
}

interface ContactVo {
  id: string;
  name: string;
  phone: string | null;
  email: string | null;
  isPrimary: boolean;
}

interface OpportunityVo {
  id: string;
  name: string;
  amountCents: number | null;
  stage: string;
}

interface ContractVo {
  id: string;
  contractNo: string;
  name: string;
  amountCents: number;
  paidCents: number;
  status: string;
}

const route = useRoute();
const router = useRouter();
const id = String(route.params.id);

const { t } = useI18n();
const query = useQuery({
  queryKey: ['crm-customer-detail', id],
  queryFn: async () => {
    const [customer, contacts, opportunities, contracts] = await Promise.all([
      http.get<CustomerVo>(`/api/crm/customers/${id}`),
      http.get<ContactVo[]>(`/api/crm/customers/${id}/contacts`),
      http.get<OpportunityVo[]>(`/api/crm/customers/${id}/opportunities`),
      http.get<ContractVo[]>(`/api/crm/customers/${id}/contracts`),
    ]);
    return { customer, contacts, opportunities, contracts };
  },
});
const customer = computed(() => query.data.value?.customer ?? null);
const contacts = computed(() => query.data.value?.contacts ?? []);
const opportunities = computed(() => query.data.value?.opportunities ?? []);
const contracts = computed(() => query.data.value?.contracts ?? []);
const loading = computed(() => query.isFetching.value);
watch(query.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('crm.loadFailed'));
});

const contactColumns = computed(() => [
  { title: t('crm.colName'), dataIndex: 'name', key: 'name' },
  { title: t('crm.colPhone'), dataIndex: 'phone', key: 'phone' },
  { title: t('crm.email'), dataIndex: 'email', key: 'email' },
  { title: t('crm.colPrimary'), dataIndex: 'isPrimary', key: 'isPrimary', width: 90 },
]);

const contractColumns = computed(() => [
  { title: t('crm.colContractNo'), dataIndex: 'contractNo', key: 'contractNo', width: 170 },
  { title: t('crm.colContractName'), dataIndex: 'name', key: 'name' },
  { title: t('crm.colAmount'), dataIndex: 'amountCents', key: 'amount', width: 130 },
  { title: t('crm.colPaid'), dataIndex: 'paidCents', key: 'paid', width: 130 },
  { title: t('crm.colStatus'), dataIndex: 'status', key: 'status', width: 90 },
]);

const contractTotal = computed(() => contracts.value.reduce((sum, c) => sum + c.amountCents, 0));
const paidTotal = computed(() => contracts.value.reduce((sum, c) => sum + c.paidCents, 0));
const collectionRate = computed(() => {
  if (contractTotal.value === 0) return '—';
  return `${Math.round((paidTotal.value / contractTotal.value) * 100)}%`;
});

const contractStatusText = (status: string): string =>
  ({ draft: t('crm.statusDraft'), active: t('crm.statusActive'), completed: t('crm.statusCompleted'), void: t('crm.statusVoid') })[status] ?? status;
const contractStatusColor = (status: string): string =>
  ({ draft: 'default', active: 'blue', completed: 'green', void: 'red' })[status] ?? 'default';

const opportunityColumns = computed(() => [
  { title: t('crm.colOppName'), dataIndex: 'name', key: 'name' },
  { title: t('crm.colAmount'), dataIndex: 'amountCents', key: 'amount', width: 140 },
  { title: t('crm.colStage'), dataIndex: 'stage', key: 'stage', width: 120 },
]);

const stageOptions = computed(() => [
  { label: t('crm.stageInitial'), value: 'initial' },
  { label: t('crm.stageProposal'), value: 'proposal' },
  { label: t('crm.stageNegotiation'), value: 'negotiation' },
  { label: t('crm.stageWon'), value: 'won' },
  { label: t('crm.stageLost'), value: 'lost' },
]);

const stageText = (stage: string): string =>
  stageOptions.value.find((s: { value: string; label: string }) => s.value === stage)?.label ?? stage;
const stageColor = (stage: string): string =>
  ({ initial: 'default', proposal: 'blue', negotiation: 'orange', won: 'green', lost: 'red' })[
    stage
  ] ?? 'default';

function formatAmount(cents: number | null): string {
  if (cents == null) return '-';
  return `¥${(cents / 100).toLocaleString('zh-CN', { minimumFractionDigits: 2 })}`;
}


</script>

<style scoped>
.page {
  padding: 0;
}

.muted {
  color: #cbd5e1;
}

.kpi-row {
  margin-bottom: 16px;
}

.kpi {
  background: #f8fafc;
}

.kpi-label {
  color: #64748b;
  font-size: 12px;
}

.kpi-value {
  font-size: 20px;
  font-weight: 600;
  margin-top: 4px;
}

.kpi-value.green {
  color: #16a34a;
}

.kpi-sub {
  color: #94a3b8;
  font-size: 12px;
}

.contract-summary {
  margin-bottom: 8px;
  color: #475569;
  font-size: 13px;
}

.contract-summary .divider {
  margin: 0 8px;
  color: #cbd5e1;
}

.contract-summary .green {
  color: #16a34a;
}
</style>
