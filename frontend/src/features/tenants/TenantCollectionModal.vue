<template>
  <a-modal
    :open="open"
    :title="t('tenants.collectionTitle')"
    :confirm-loading="saving"
    @ok="save"
    @update:open="emit('update:open', $event)"
  >
    <a-form layout="vertical">
      <a-form-item :label="t('tenants.collectionNote')" required>
        <a-textarea v-model:value="form.note" :rows="3" />
      </a-form-item>
      <a-form-item :label="t('tenants.promisedAmount')">
        <a-input-number v-model:value="form.promisedAmountCents" :min="0" style="width: 100%" />
      </a-form-item>
      <a-form-item :label="t('tenants.promisedDate')">
        <a-input v-model:value="form.promisedDate" placeholder="2026-12-31" />
      </a-form-item>
    </a-form>
    <a-divider />
    <div class="section-title">{{ t('tenants.history') }}</div>
    <a-table
      :data-source="collections"
      :columns="columns"
      :loading="loading"
      size="small"
      :pagination="false"
      row-key="id"
    />
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 账单催收记录弹窗（新增 + 历史）
//
// 每次打开都重新拉取该账单的催收历史，避免展示过期数据。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';
import type { BillCollection } from './types';

const props = defineProps<{ open: boolean; billId: string }>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

const { t } = useI18n();
const loading = ref(false);
const saving = ref(false);
const collections = ref<BillCollection[]>([]);
const form = reactive({ note: '', promisedAmountCents: 0, promisedDate: '' });

const columns = computed(() => [
  { title: t('tenants.colTime'), dataIndex: 'createdAt', key: 'createdAt', width: 180 },
  { title: t('tenants.colNote'), dataIndex: 'note', key: 'note' },
  { title: t('tenants.colPromisedAmount'), dataIndex: 'promisedAmountCents', key: 'promisedAmountCents', width: 110 },
  { title: t('tenants.colPromisedDate'), dataIndex: 'promisedDate', key: 'promisedDate', width: 110 },
]);

async function load(): Promise<void> {
  if (!props.billId) return;
  loading.value = true;
  try {
    collections.value = await http.get<BillCollection[]>(
      `/api/system/tenants/bills/${props.billId}/collections`,
    );
  } catch {
    collections.value = [];
  } finally {
    loading.value = false;
  }
}

async function save(): Promise<void> {
  saving.value = true;
  try {
    await http.post(`/api/system/tenants/bills/${props.billId}/collections`, {
      note: form.note,
      promisedAmountCents: form.promisedAmountCents || null,
      promisedDate: form.promisedDate || null,
    });
    message.success(t('tenants.collectionSaved'));
    form.note = '';
    form.promisedAmountCents = 0;
    form.promisedDate = '';
    await load();
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('tenants.saveFailed'));
  } finally {
    saving.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    form.note = '';
    form.promisedAmountCents = 0;
    form.promisedDate = '';
    void load();
  },
);
</script>
