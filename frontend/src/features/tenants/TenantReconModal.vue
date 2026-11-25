<template>
  <a-modal
    :open="open"
    :title="t('tenants.reconTitle')"
    :footer="null"
    width="900px"
    @update:open="emit('update:open', $event)"
  >
    <a-table
      :data-source="rows"
      :columns="columns"
      :loading="loading"
      size="small"
      row-key="tenantId"
      :pagination="{ pageSize: 10 }"
    />
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 账单对账弹窗
// =============================================================================

import { computed, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';
import type { ReconciliationRow } from './types';

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

const { t } = useI18n();
const loading = ref(false);
const rows = ref<ReconciliationRow[]>([]);

const columns = computed(() => [
  { title: t('tenants.colTenant'), dataIndex: 'tenantName', key: 'tenantName' },
  { title: t('tenants.colBillCount'), dataIndex: 'billCount', key: 'billCount', width: 70 },
  { title: t('tenants.colTotal'), dataIndex: 'totalCents', key: 'totalCents', width: 100 },
  { title: t('tenants.colPaid'), dataIndex: 'paidCents', key: 'paidCents', width: 100 },
  { title: t('tenants.colRefunded'), dataIndex: 'refundedCents', key: 'refundedCents', width: 100 },
  { title: t('tenants.colOutstanding'), dataIndex: 'outstandingCents', key: 'outstandingCents', width: 100 },
]);

async function load(): Promise<void> {
  loading.value = true;
  try {
    rows.value = await http.get<ReconciliationRow[]>(
      '/api/system/tenants/bills/reconciliation',
    );
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('tenants.reconLoadFailed'));
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
