<template>
  <a-modal
    :open="open"
    :title="contract ? t('crm.editContract') : t('crm.newContract')"
    :confirm-loading="submitting"
    @ok="submit"
    @update:open="emit('update:open', $event)"
  >
    <a-form ref="formRef" :model="form" :rules="rules" layout="vertical">
      <a-form-item :label="t('crm.customer')" name="customerId">
        <a-select
          v-model:value="form.customerId"
          :options="customerOptions"
          :disabled="!!contract"
          show-search
          option-filter-prop="label"
          :placeholder="t('crm.customerPlaceholder')"
        />
      </a-form-item>
      <a-form-item :label="t('crm.linkedOpportunity')" name="opportunityId">
        <a-select
          v-model:value="form.opportunityId"
          :options="opportunityOptions"
          allow-clear
          show-search
          option-filter-prop="label"
          :placeholder="t('crm.opportunityPlaceholder')"
        />
      </a-form-item>
      <a-form-item :label="t('crm.contractNo')" name="contractNo">
        <a-input v-model:value="form.contractNo" :placeholder="t('crm.contractNoPlaceholder')" />
      </a-form-item>
      <a-form-item :label="t('crm.contractName')" name="name">
        <a-input v-model:value="form.name" :placeholder="t('crm.contractNamePlaceholder')" />
      </a-form-item>
      <a-form-item :label="t('crm.amountYuan')" name="amount">
        <a-input-number v-model:value="form.amount" :min="0" :precision="2" style="width: 200px" />
      </a-form-item>
      <a-form-item :label="t('crm.status')" name="status">
        <a-select v-model:value="form.status" :options="statusOptions" />
      </a-form-item>
    </a-form>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 合同新建/编辑弹窗
//
// 编辑时不允许更换客户（避免历史对账错乱）；客户/商机下拉在打开时加载。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { FormInstance } from 'ant-design-vue';
import { ApiError, http } from '../../api/client';

interface ContractVo {
  id: string;
  customerId: string;
  contractNo: string;
  name: string;
  amountCents: number;
  status: string;
}

const props = defineProps<{ open: boolean; contract: ContractVo | null }>();
const emit = defineEmits<{
  (e: 'update:open', v: boolean): void;
  (e: 'saved'): void;
}>();

const { t } = useI18n();
const submitting = ref(false);
const formRef = ref<FormInstance>();
const customers = ref<{ id: string; name: string }[]>([]);
const opportunities = ref<{ id: string; name: string; customerId: string }[]>([]);

const customerOptions = computed(() => customers.value.map((c) => ({ label: c.name, value: c.id })));
const opportunityOptions = computed(() =>
  opportunities.value
    .filter((o) => !form.customerId || o.customerId === form.customerId)
    .map((o) => ({ label: o.name, value: o.id })),
);

const statusOptions = computed(() => [
  { label: t('crm.statusDraft'), value: 'draft' },
  { label: t('crm.statusActive'), value: 'active' },
  { label: t('crm.statusCompleted'), value: 'completed' },
  { label: t('crm.statusVoid'), value: 'void' },
]);

const form = reactive({
  customerId: undefined as string | undefined,
  opportunityId: undefined as string | undefined,
  contractNo: '',
  name: '',
  amount: 0,
  status: 'draft',
});

const rules = {
  customerId: [{ required: true, message: t('crm.selectCustomer') }],
  contractNo: [{ required: true, max: 64, message: t('crm.contractNoRule') }],
  name: [{ required: true, max: 128, message: t('crm.oppNameRule') }],
};

async function loadOptions(): Promise<void> {
  const [c, o] = await Promise.all([
    http.get<{ data: { id: string; name: string }[] }>('/api/crm/customers?page=1&size=200'),
    http.get<{ data: { id: string; name: string; customerId: string }[] }>(
      '/api/crm/opportunities?page=1&size=200',
    ),
  ]);
  customers.value = c.data;
  opportunities.value = o.data;
}

async function submit(): Promise<void> {
  try {
    await formRef.value?.validate();
  } catch {
    return;
  }
  submitting.value = true;
  try {
    if (props.contract) {
      await http.patch(`/api/crm/contracts/${props.contract.id}`, {
        opportunityId: form.opportunityId ?? null,
        contractNo: form.contractNo.trim(),
        name: form.name.trim(),
        amountCents: Math.round(form.amount * 100),
        status: form.status,
      });
    } else {
      await http.post('/api/crm/contracts', {
        customerId: form.customerId,
        opportunityId: form.opportunityId ?? null,
        contractNo: form.contractNo.trim(),
        name: form.name.trim(),
        amountCents: Math.round(form.amount * 100),
        status: form.status,
      });
    }
    message.success(t('crm.saved'));
    emit('update:open', false);
    emit('saved');
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
    form.customerId = props.contract?.customerId ?? undefined;
    form.opportunityId = undefined;
    form.contractNo = props.contract?.contractNo ?? '';
    form.name = props.contract?.name ?? '';
    form.amount = props.contract ? props.contract.amountCents / 100 : 0;
    form.status = props.contract?.status ?? 'draft';
    loadOptions().catch(() => {
      message.error(t('crm.contractLoadFailed'));
    });
  },
);
</script>
