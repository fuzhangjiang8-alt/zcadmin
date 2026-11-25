<template>
  <a-modal
    :open="open"
    :title="t('tenants.billingTitle')"
    :confirm-loading="saving"
    @ok="save"
    @update:open="emit('update:open', $event)"
  >
    <a-form layout="vertical">
      <a-form-item :label="t('tenants.pricePerUser')">
        <a-input-number v-model:value="form.pricePerUserMonthCents" :min="0" style="width: 100%" />
      </a-form-item>
      <a-form-item :label="t('tenants.pricePerGb')">
        <a-input-number v-model:value="form.pricePerGbMonthCents" :min="0" style="width: 100%" />
      </a-form-item>
      <a-form-item :label="t('tenants.currency')">
        <a-input v-model:value="form.currency" :maxlength="3" />
      </a-form-item>
    </a-form>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 租户计费配置弹窗
// =============================================================================

import { reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';
import type { TenantVo } from './types';

const props = defineProps<{ open: boolean; tenant: TenantVo | null }>();
const emit = defineEmits<{
  (e: 'update:open', v: boolean): void;
  (e: 'saved'): void;
}>();

const { t } = useI18n();
const saving = ref(false);
const form = reactive({
  id: '',
  pricePerUserMonthCents: 0,
  pricePerGbMonthCents: 0,
  currency: 'CNY',
});

watch(
  () => props.open,
  (open) => {
    if (open && props.tenant) {
      form.id = props.tenant.id;
      form.pricePerUserMonthCents = props.tenant.pricePerUserMonthCents;
      form.pricePerGbMonthCents = props.tenant.pricePerGbMonthCents;
      form.currency = props.tenant.currency;
    }
  },
);

async function save(): Promise<void> {
  saving.value = true;
  try {
    await http.put(`/api/system/tenants/${form.id}/billing`, {
      pricePerUserMonthCents: form.pricePerUserMonthCents,
      pricePerGbMonthCents: form.pricePerGbMonthCents,
      currency: form.currency,
    });
    message.success(t('tenants.billingUpdated'));
    emit('update:open', false);
    emit('saved');
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('tenants.updateFailed'));
  } finally {
    saving.value = false;
  }
}
</script>
