<template>
  <a-modal
    :open="open"
    :title="t('tenants.quotaTitle')"
    :confirm-loading="saving"
    @ok="save"
    @update:open="emit('update:open', $event)"
  >
    <a-form layout="vertical">
      <a-form-item :label="t('tenants.maxUsersLabel')">
        <a-input-number v-model:value="form.maxUsers" :min="0" :max="100000" style="width: 100%" />
      </a-form-item>
      <a-form-item :label="t('tenants.maxStorageLabel')">
        <a-input-number v-model:value="form.maxStorageMb" :min="0" style="width: 100%" />
      </a-form-item>
    </a-form>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 租户配额弹窗
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
const form = reactive({ id: '', maxUsers: 0, maxStorageMb: 0 });

watch(
  () => props.open,
  (open) => {
    if (open && props.tenant) {
      form.id = props.tenant.id;
      form.maxUsers = props.tenant.maxUsers;
      form.maxStorageMb = Math.round(props.tenant.maxStorageBytes / 1024 / 1024);
    }
  },
);

async function save(): Promise<void> {
  saving.value = true;
  try {
    await http.put(`/api/system/tenants/${form.id}/quota`, {
      maxUsers: form.maxUsers,
      maxStorageBytes: Math.round(form.maxStorageMb * 1024 * 1024),
    });
    message.success(t('tenants.quotaUpdated'));
    emit('update:open', false);
    emit('saved');
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('tenants.updateFailed'));
  } finally {
    saving.value = false;
  }
}
</script>
