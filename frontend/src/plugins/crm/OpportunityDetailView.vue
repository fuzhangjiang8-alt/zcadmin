<template>
  <div class="page">
    <a-card :bordered="false">
      <template #title>
        <a-space>
          <a-button type="link" size="small" @click="router.back()">{{ t('crm.back') }}</a-button>
          <span>{{ opportunity?.name ?? t('crm.opportunityDetail') }}</span>
        </a-space>
      </template>
      <template #extra>
        <a-button v-perm="PERMS.CONTRACT_ADD" type="primary" @click="openConvert">
           {{ t('crm.convertToContract') }}
        </a-button>
      </template>

      <a-spin :spinning="loading">
        <a-descriptions v-if="opportunity" :column="3" size="small" bordered>
          <a-descriptions-item :label="t('crm.oppName')">{{ opportunity.name }}</a-descriptions-item>
          <a-descriptions-item :label="t('crm.customer')">{{ customerName }}</a-descriptions-item>
          <a-descriptions-item :label="t('crm.stage')">
            <a-tag :color="stageColor(opportunity.stage)">{{ stageText(opportunity.stage) }}</a-tag>
          </a-descriptions-item>
          <a-descriptions-item :label="t('crm.colAmount')">{{ formatAmount(opportunity.amountCents) }}</a-descriptions-item>
          <a-descriptions-item :label="t('crm.expectedClose')">
            {{ opportunity.expectedClose ?? '-' }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('crm.convertedContractLabel')">{{ t('crm.convertedContracts', { n: contracts.length }) }}</a-descriptions-item>
        </a-descriptions>

        <a-divider orientation="left">{{ t('crm.convertedContractTitle') }}</a-divider>
        <a-table
          size="small"
          :columns="columns"
          :data-source="contracts"
          row-key="id"
          :pagination="false"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'contractNo'">
              <router-link :to="{ name: 'crm-contract-detail', params: { id: record.id } }">
                {{ record.contractNo }}
              </router-link>
            </template>
            <template v-else-if="column.key === 'amount'">{{ formatAmount(record.amountCents) }}</template>
            <template v-else-if="column.key === 'paid'">{{ formatAmount(record.paidCents) }}</template>
            <template v-else-if="column.key === 'status'">
              <a-tag :color="contractStatusColor(record.status)">
                {{ contractStatusText(record.status) }}
              </a-tag>
            </template>
          </template>
        </a-table>
        <div v-if="contracts.length === 0" class="empty">
          {{ t('crm.noContractsHint') }}
        </div>

        <a-divider orientation="left">{{ t('crm.stageHistory') }}</a-divider>
        <div v-if="stageHistory.length > 0" class="history">
          <div v-for="h in stageHistory" :key="h.changedAt + h.toStage" class="history-row">
            <a-tag>{{ h.fromStage ? stageText(h.fromStage) : t('crm.created') }}</a-tag>
            <span class="arrow">→</span>
            <a-tag color="blue">{{ stageText(h.toStage) }}</a-tag>
            <span class="muted">{{ h.changedAt }}</span>
          </div>
        </div>
        <div v-else class="empty">{{ t('crm.noStageHistory') }}</div>
      </a-spin>
    </a-card>

    <a-modal
      v-model:open="convertOpen"
      :title="t('crm.convertTitle')"
      :confirm-loading="converting"
      :ok-text="t('crm.okCreateContract')"
      @ok="submitConvert"
    >
      <a-form ref="convertFormRef" :model="convertForm" :rules="convertRules" layout="vertical">
        <a-form-item :label="t('crm.colContractNo')" name="contractNo">
          <a-input v-model:value="convertForm.contractNo" :placeholder="t('crm.contractNoPlaceholder')" />
        </a-form-item>
        <a-form-item :label="t('crm.colContractName')" name="name">
          <a-input v-model:value="convertForm.name" />
        </a-form-item>
        <a-form-item :label="t('crm.amountYuan')" name="amount">
          <a-input-number v-model:value="convertForm.amount" :min="0" :precision="2" style="width: 200px" />
        </a-form-item>
      </a-form>
    </a-modal>
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../../permissions';
// =============================================================================
// 商机详情：商机信息 + 由它转化出的合同
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { useQuery, useQueryClient } from '@tanstack/vue-query';
import { useI18n } from 'vue-i18n';
import { useRoute, useRouter } from 'vue-router';
import { message } from 'ant-design-vue';
import type { FormInstance } from 'ant-design-vue';
import { ApiError, http } from '../../api/client';

interface OpportunityVo {
  id: string;
  customerId: string;
  name: string;
  amountCents: number | null;
  stage: string;
  expectedClose: string | null;
}

interface ContractVo {
  id: string;
  contractNo: string;
  name: string;
  amountCents: number;
  paidCents: number;
  status: string;
}

interface StageHistory {
  fromStage: string | null;
  toStage: string;
  changedAt: string;
}

interface CustomerVo {
  id: string;
  name: string;
}

const route = useRoute();
const router = useRouter();
const id = String(route.params.id);

const { t } = useI18n();
const queryClient = useQueryClient();
const query = useQuery({
  queryKey: ['crm-opportunity-detail', id],
  queryFn: async () => {
    const [opportunity, contracts, stageHistory] = await Promise.all([
      http.get<OpportunityVo>(`/api/crm/opportunities/${id}`),
      http.get<ContractVo[]>(`/api/crm/opportunities/${id}/contracts`),
      http.get<StageHistory[]>(`/api/crm/opportunities/${id}/stage-history`),
    ]);
    let customer: CustomerVo | null = null;
    try {
      customer = await http.get<CustomerVo>(`/api/crm/customers/${opportunity.customerId}`);
    } catch {
      // 客户不可见时保留 id 展示
    }
    return { opportunity, contracts, stageHistory, customer };
  },
});
const opportunity = computed(() => query.data.value?.opportunity ?? null);
const contracts = computed(() => query.data.value?.contracts ?? []);
const stageHistory = computed(() => query.data.value?.stageHistory ?? []);
const customer = computed(() => query.data.value?.customer ?? null);
const loading = computed(() => query.isFetching.value);
watch(query.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('crm.loadFailed'));
});

const columns = computed(() => [
  { title: t('crm.colContractNo'), dataIndex: 'contractNo', key: 'contractNo', width: 180 },
  { title: t('crm.colContractName'), dataIndex: 'name', key: 'name' },
  { title: t('crm.colAmount'), dataIndex: 'amountCents', key: 'amount', width: 140 },
  { title: t('crm.colPaid'), dataIndex: 'paidCents', key: 'paid', width: 140 },
  { title: t('crm.colStatus'), dataIndex: 'status', key: 'status', width: 100 },
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

const contractStatusText = (status: string): string =>
  ({ draft: t('crm.statusDraft'), active: t('crm.statusActive'), completed: t('crm.statusCompleted'), void: t('crm.statusVoid') })[status] ?? status;
const contractStatusColor = (status: string): string =>
  ({ draft: 'default', active: 'blue', completed: 'green', void: 'red' })[status] ?? 'default';

const customerName = computed(() => customer.value?.name ?? opportunity.value?.customerId ?? '-');

function formatAmount(cents: number | null): string {
  if (cents == null) return '-';
  return `¥${(cents / 100).toLocaleString('zh-CN', { minimumFractionDigits: 2 })}`;
}

// ---- 转化为合同 ----
const convertOpen = ref(false);
const converting = ref(false);
const convertFormRef = ref<FormInstance>();
const convertForm = reactive({ contractNo: '', name: '', amount: 0 });
const convertRules = {
  contractNo: [{ required: true, max: 64, message: t('crm.contractNoRule') }],
  name: [{ required: true, max: 128, message: t('crm.oppNameRule') }],
};

function openConvert(): void {
  if (!opportunity.value) return;
  convertForm.contractNo = '';
  convertForm.name = opportunity.value.name;
  convertForm.amount = (opportunity.value.amountCents ?? 0) / 100;
  convertOpen.value = true;
}

async function submitConvert(): Promise<void> {
  if (!opportunity.value) return;
  try {
    await convertFormRef.value?.validate();
  } catch {
    return;
  }
  converting.value = true;
  try {
    await http.post('/api/crm/contracts', {
      customerId: opportunity.value.customerId,
      opportunityId: opportunity.value.id,
      contractNo: convertForm.contractNo.trim(),
      name: convertForm.name.trim(),
      amountCents: Math.round(convertForm.amount * 100),
      status: 'draft',
    });
    message.success(t('crm.contractCreated'));
    convertOpen.value = false;
    void queryClient.invalidateQueries({ queryKey: ['crm-opportunity-detail'] });
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('crm.createFailed'));
  } finally {
    converting.value = false;
  }
}


</script>

<style scoped>
.page {
  padding: 0;
}

.empty {
  color: #94a3b8;
  padding: 12px 0;
}

.history-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 0;
}

.arrow {
  color: #94a3b8;
}
</style>
