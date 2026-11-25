<template>
  <a-modal
    :open="open"
    :title="opportunity ? t('crm.editOpportunity') : t('crm.newOpportunity')"
    :confirm-loading="submitting"
    @ok="submit"
    @update:open="emit('update:open', $event)"
  >
    <a-form ref="formRef" :model="form" :rules="rules" layout="vertical">
      <a-form-item :label="t('crm.customer')" name="customerId">
        <a-select
          v-model:value="form.customerId"
          :options="customerOptions"
          :disabled="!!opportunity"
          show-search
          option-filter-prop="label"
          :placeholder="t('crm.customerPlaceholder')"
        />
      </a-form-item>
      <a-form-item :label="t('crm.oppName')" name="name">
        <a-input v-model:value="form.name" :placeholder="t('crm.oppNamePlaceholder')" />
      </a-form-item>
      <a-form-item :label="t('crm.amountYuan')" name="amount">
        <a-input-number v-model:value="form.amount" :min="0" :precision="2" style="width: 200px" />
      </a-form-item>
      <a-form-item :label="t('crm.stage')" name="stage">
        <a-select v-model:value="form.stage" :options="stageOptions" />
      </a-form-item>
    </a-form>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 商机新建/编辑弹窗
//
// 编辑时不允许更换客户；保存成功后把最新商机 emit 给父页面，
// 由父页面处理「赢单 → 引导创建合同」的跳转。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { FormInstance } from 'ant-design-vue';
import { ApiError, http } from '../../api/client';

interface OpportunityVo {
  id: string;
  customerId: string;
  name: string;
  amountCents: number | null;
  stage: string;
}

const props = defineProps<{ open: boolean; opportunity: OpportunityVo | null }>();
const emit = defineEmits<{
  (e: 'update:open', v: boolean): void;
  (e: 'saved', value: OpportunityVo): void;
}>();

const { t } = useI18n();
const submitting = ref(false);
const formRef = ref<FormInstance>();
const customers = ref<{ id: string; name: string }[]>([]);
const customerOptions = computed(() => customers.value.map((c) => ({ label: c.name, value: c.id })));

const stageOptions = computed(() => [
  { label: t('crm.stageInitial'), value: 'initial' },
  { label: t('crm.stageProposal'), value: 'proposal' },
  { label: t('crm.stageNegotiation'), value: 'negotiation' },
  { label: t('crm.stageWon'), value: 'won' },
  { label: t('crm.stageLost'), value: 'lost' },
]);

const form = reactive({
  customerId: undefined as string | undefined,
  name: '',
  amount: 0,
  stage: 'initial',
});

const rules = {
  customerId: [{ required: true, message: t('crm.selectCustomer') }],
  name: [{ required: true, max: 128, message: t('crm.oppNameRule') }],
};

async function loadCustomers(): Promise<void> {
  const result = await http.get<{ data: { id: string; name: string }[] }>(
    '/api/crm/customers?page=1&size=200',
  );
  customers.value = result.data;
}

async function submit(): Promise<void> {
  try {
    await formRef.value?.validate();
  } catch {
    return;
  }
  submitting.value = true;
  try {
    const payload = {
      customerId: form.customerId,
      name: form.name.trim(),
      amountCents: Math.round(form.amount * 100),
      stage: form.stage,
    };
    const saved = props.opportunity
      ? await http.patch<OpportunityVo>(`/api/crm/opportunities/${props.opportunity.id}`, {
          name: payload.name,
          amountCents: payload.amountCents,
          stage: payload.stage,
        })
      : await http.post<OpportunityVo>('/api/crm/opportunities', payload);
    message.success(t('crm.saved'));
    emit('update:open', false);
    emit('saved', saved);
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('crm.saveFailed'));
  } finally {
    submitting.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    form.customerId = props.opportunity?.customerId ?? undefined;
    form.name = props.opportunity?.name ?? '';
    form.amount = (props.opportunity?.amountCents ?? 0) / 100;
    form.stage = props.opportunity?.stage ?? 'initial';
    loadCustomers().catch(() => {
      message.error(t('crm.loadCustomersFailed'));
    });
  },
);
</script>
