<template>
  <a-modal
    :open="open"
    :title="t('tenants.billsTitle')"
    :footer="null"
    width="820px"
    @update:open="emit('update:open', $event)"
  >
    <a-table
      :data-source="bills"
      :columns="columns"
      :loading="loading"
      size="small"
      row-key="id"
      :pagination="{ pageSize: 10 }"
    >
      <template #bodyCell="{ column, record }">
        <template v-if="column.key === 'status'">
          <a-tag
            :color="record.status === 'paid' ? 'green' : record.status === 'void' ? 'default' : 'blue'"
          >
            {{ billStatusText(record.status) }}
          </a-tag>
        </template>
        <template v-else-if="column.key === 'actions'">
          <a-button
            v-if="record.status === 'draft'"
            type="link"
            size="small"
            @click="updateStatus(record, 'issued')"
          >
            {{ t('tenants.issue') }}
          </a-button>
          <a-button
            v-if="record.status === 'issued'"
            type="link"
            size="small"
            @click="openPayment(record)"
          >
            {{ t('tenants.receive') }}
          </a-button>
          <a-button
            v-if="record.status === 'issued' || record.status === 'paid'"
            type="link"
            size="small"
            @click="openCollection(record)"
          >
            {{ t('tenants.collect') }}
          </a-button>
          <a-button
            v-if="record.status === 'issued' || record.status === 'paid'"
            type="link"
            size="small"
            @click="openRefund(record)"
          >
            {{ t('tenants.refund') }}
          </a-button>
          <a-button
            v-if="record.status === 'draft' || record.status === 'issued'"
            type="link"
            size="small"
            danger
            @click="updateStatus(record, 'void')"
          >
            {{ t('tenants.void') }}
          </a-button>
        </template>
      </template>
    </a-table>

    <a-modal
      v-model:open="paymentOpen"
      :title="t('tenants.paymentTitle')"
      :confirm-loading="paymentSaving"
      @ok="savePayment"
    >
      <a-form layout="vertical">
        <a-form-item :label="t('tenants.paymentAmount')">
          <a-input-number v-model:value="paymentForm.amountCents" :min="1" style="width: 100%" />
        </a-form-item>
        <a-form-item :label="t('tenants.method')">
          <a-select v-model:value="paymentForm.method">
            <a-select-option value="bank">{{ t('tenants.methodBank') }}</a-select-option>
            <a-select-option value="cash">{{ t('tenants.methodCash') }}</a-select-option>
            <a-select-option value="other">{{ t('tenants.methodOther') }}</a-select-option>
          </a-select>
        </a-form-item>
        <a-form-item :label="t('tenants.reference')">
          <a-input v-model:value="paymentForm.reference" />
        </a-form-item>
      </a-form>
    </a-modal>

    <a-modal
      v-model:open="refundOpen"
      :title="t('tenants.refundTitle')"
      :confirm-loading="refundSaving"
      @ok="saveRefund"
    >
      <a-form layout="vertical">
        <a-form-item :label="t('tenants.refundAmount')">
          <a-input-number v-model:value="refundForm.amountCents" :min="1" style="width: 100%" />
        </a-form-item>
        <a-form-item :label="t('tenants.refundReason')">
          <a-input v-model:value="refundForm.reason" />
        </a-form-item>
      </a-form>
    </a-modal>

    <TenantCollectionModal v-model:open="collectionOpen" :bill-id="collectionBillId" />
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 租户账单弹窗（含收款/退款/催收）
//
// 账单是整个计费域的核心实体：出账、收款、退款、催收、作废都在这里完成；
// 每次写操作后重新拉取列表，避免前端手改本地状态导致与后端不一致。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';
import TenantCollectionModal from './TenantCollectionModal.vue';
import type { TenantBillRow } from './types';

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

const { t } = useI18n();

const billStatusMap: Record<string, string> = {
  draft: 'tenants.billStatusDraft',
  issued: 'tenants.billStatusIssued',
  paid: 'tenants.billStatusPaid',
  void: 'tenants.billStatusVoid',
};
function billStatusText(status: string): string {
  return billStatusMap[status] ? t(billStatusMap[status]) : status;
}

const loading = ref(false);
const bills = ref<TenantBillRow[]>([]);

const paymentOpen = ref(false);
const paymentSaving = ref(false);
const paymentForm = reactive({ billId: '', amountCents: 1, method: 'bank', reference: '' });

const refundOpen = ref(false);
const refundSaving = ref(false);
const refundForm = reactive({ billId: '', amountCents: 1, reason: '' });

const collectionOpen = ref(false);
const collectionBillId = ref("");

const columns = computed(() => [
  { title: t('tenants.colTenant'), dataIndex: 'tenantName', key: 'tenantName' },
  { title: t('tenants.colMonth'), dataIndex: 'billMonth', key: 'billMonth', width: 110 },
  { title: t('tenants.colUsers'), dataIndex: 'avgUsers', key: 'avgUsers', width: 80 },
  { title: t('tenants.colAmount'), dataIndex: 'totalCents', key: 'totalCents', width: 110 },
  { title: t('tenants.colStatus'), dataIndex: 'status', key: 'status', width: 90 },
  { title: t('tenants.colActions'), key: 'actions', width: 150 },
]);

async function reload(): Promise<void> {
  loading.value = true;
  try {
    bills.value = await http.get<TenantBillRow[]>('/api/system/tenants/bills');
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('tenants.billLoadFailed'));
  } finally {
    loading.value = false;
  }
}

async function updateStatus(row: TenantBillRow, status: string): Promise<void> {
  try {
    await http.put(`/api/system/tenants/bills/${row.id}/status`, { status });
    message.success(t('tenants.billStatusUpdated'));
    await reload();
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('tenants.billUpdateFailed'));
  }
}

function openPayment(row: TenantBillRow): void {
  paymentForm.billId = row.id;
  paymentForm.amountCents = Math.max(1, row.totalCents - row.paidCents);
  paymentForm.method = 'bank';
  paymentForm.reference = '';
  paymentOpen.value = true;
}

async function savePayment(): Promise<void> {
  paymentSaving.value = true;
  try {
    await http.post(`/api/system/tenants/bills/${paymentForm.billId}/payments`, {
      amountCents: paymentForm.amountCents,
      method: paymentForm.method,
      reference: paymentForm.reference || null,
    });
    message.success(t('tenants.paymentSaved'));
    paymentOpen.value = false;
    await reload();
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('tenants.paymentFailed'));
  } finally {
    paymentSaving.value = false;
  }
}

function openRefund(row: TenantBillRow): void {
  refundForm.billId = row.id;
  refundForm.amountCents = Math.max(1, row.paidCents);
  refundForm.reason = '';
  refundOpen.value = true;
}

async function saveRefund(): Promise<void> {
  refundSaving.value = true;
  try {
    await http.post(`/api/system/tenants/bills/${refundForm.billId}/refunds`, {
      amountCents: refundForm.amountCents,
      reason: refundForm.reason,
    });
    message.success(t('tenants.refundSaved'));
    refundOpen.value = false;
    await reload();
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('tenants.refundFailed'));
  } finally {
    refundSaving.value = false;
  }
}

function openCollection(row: TenantBillRow): void {
  collectionBillId.value = row.id;
  collectionOpen.value = true;
}

watch(
  () => props.open,
  (open) => {
    if (open) void reload();
  },
);
</script>
