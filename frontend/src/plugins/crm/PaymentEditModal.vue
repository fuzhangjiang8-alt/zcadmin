<template>
  <a-modal
    :open="open"
    :title="payment ? t('crm.editPayment') : t('crm.newPayment')"
    :confirm-loading="submitting"
    @ok="submit"
    @update:open="emit('update:open', $event)"
  >
    <a-form ref="formRef" :model="form" :rules="rules" layout="vertical">
      <a-form-item :label="t('crm.contract')" name="contractId">
        <a-select
          v-model:value="form.contractId"
          :options="contractOptions"
          :disabled="!!payment"
          show-search
          option-filter-prop="label"
          :placeholder="t('crm.contractPlaceholder')"
        />
      </a-form-item>
      <a-form-item :label="t('crm.paymentNo')" name="paymentNo">
        <a-input v-model:value="form.paymentNo" :placeholder="t('crm.paymentNoPlaceholder')" />
      </a-form-item>
      <a-form-item :label="t('crm.amountYuan')" name="amount">
        <a-input-number v-model:value="form.amount" :min="0" :precision="2" style="width: 200px" />
      </a-form-item>
      <a-form-item :label="t('crm.method')" name="method">
        <a-select v-model:value="form.method" :options="methodOptions" allow-clear />
      </a-form-item>
    </a-form>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 回款登记/编辑弹窗
//
// 编辑时不允许更换合同；合同下拉在打开时加载（最多 200 条）。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { FormInstance } from 'ant-design-vue';
import { ApiError, http } from '../../api/client';

interface PaymentVo {
  id: string;
  contractId: string;
  paymentNo: string;
  amountCents: number;
  method: string | null;
}

const props = defineProps<{ open: boolean; payment: PaymentVo | null }>();
const emit = defineEmits<{
  (e: 'update:open', v: boolean): void;
  (e: 'saved'): void;
}>();

const { t } = useI18n();
const submitting = ref(false);
const formRef = ref<FormInstance>();
const contracts = ref<{ id: string; contractNo: string; name: string }[]>([]);
const contractOptions = computed(() =>
  contracts.value.map((c) => ({ label: `${c.contractNo} · ${c.name}`, value: c.id })),
);

const methodOptions = computed(() => [
  { label: t('crm.methodTransfer'), value: 'transfer' },
  { label: t('crm.methodAcceptance'), value: 'acceptance' },
  { label: t('crm.methodCash'), value: 'cash' },
  { label: t('crm.methodOther'), value: 'other' },
]);

const form = reactive({
  contractId: undefined as string | undefined,
  paymentNo: '',
  amount: 0,
  method: 'transfer' as string | undefined,
});

const rules = {
  contractId: [{ required: true, message: t('crm.selectContract') }],
  paymentNo: [{ required: true, max: 64, message: t('crm.paymentNoRule') }],
};

async function loadContracts(): Promise<void> {
  const result = await http.get<{ data: { id: string; contractNo: string; name: string }[] }>(
    '/api/crm/contracts?page=1&size=200',
  );
  contracts.value = result.data;
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
      contractId: form.contractId,
      paymentNo: form.paymentNo.trim(),
      amountCents: Math.round(form.amount * 100),
      method: form.method ?? null,
    };
    if (props.payment) {
      await http.patch(`/api/crm/payments/${props.payment.id}`, {
        paymentNo: payload.paymentNo,
        amountCents: payload.amountCents,
        method: payload.method,
      });
    } else {
      await http.post('/api/crm/payments', payload);
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
    form.contractId = props.payment?.contractId ?? undefined;
    form.paymentNo = props.payment?.paymentNo ?? '';
    form.amount = props.payment ? props.payment.amountCents / 100 : 0;
    form.method = props.payment?.method ?? 'transfer';
    loadContracts().catch(() => {
      message.error(t('crm.loadContractsFailed'));
    });
  },
);
</script>
