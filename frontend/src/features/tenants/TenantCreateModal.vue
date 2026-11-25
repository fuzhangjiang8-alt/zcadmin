<template>
  <a-modal
    :open="open"
    :title="t('tenants.createTitle')"
    :confirm-loading="saving"
    @ok="submit"
    @update:open="emit('update:open', $event)"
  >
    <a-form layout="vertical">
      <a-form-item :label="t('tenants.code')" required>
        <a-input v-model:value="form.code" :placeholder="t('tenants.codePlaceholder')" />
      </a-form-item>
      <a-form-item :label="t('tenants.nameLabel')" required>
        <a-input v-model:value="form.name" />
      </a-form-item>
      <a-form-item :label="t('tenants.adminUsername')" required>
        <a-input v-model:value="form.adminUsername" />
      </a-form-item>
      <a-form-item :label="t('tenants.adminDisplayName')" required>
        <a-input v-model:value="form.adminDisplayName" />
      </a-form-item>
      <a-form-item :label="t('tenants.adminPassword')" required>
        <a-input-password v-model:value="form.adminPassword" />
      </a-form-item>
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
// 新建租户弹窗（含初始管理员）
//
// 后端会同时创建 tenant_admin 角色并授予全部租户级权限。
// =============================================================================

import { reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{
  (e: 'update:open', v: boolean): void;
  (e: 'saved'): void;
}>();

const { t } = useI18n();
const saving = ref(false);
const form = reactive({
  code: '',
  name: '',
  adminUsername: '',
  adminDisplayName: '',
  adminPassword: '',
  maxUsers: 0,
  maxStorageMb: 0,
});

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    form.code = '';
    form.name = '';
    form.adminUsername = '';
    form.adminDisplayName = '';
    form.adminPassword = '';
    form.maxUsers = 0;
    form.maxStorageMb = 0;
  },
);

async function submit(): Promise<void> {
  saving.value = true;
  try {
    await http.post('/api/system/tenants', {
      code: form.code.trim(),
      name: form.name.trim(),
      adminUsername: form.adminUsername.trim(),
      adminDisplayName: form.adminDisplayName.trim(),
      adminPassword: form.adminPassword,
      maxUsers: form.maxUsers,
      maxStorageBytes: Math.round(form.maxStorageMb * 1024 * 1024),
    });
    message.success(t('tenants.created'));
    emit('update:open', false);
    emit('saved');
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('tenants.createFailed'));
  } finally {
    saving.value = false;
  }
}
</script>
