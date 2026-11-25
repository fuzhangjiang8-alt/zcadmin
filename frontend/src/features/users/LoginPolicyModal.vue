<template>
  <a-modal
    :open="open"
    :title="t('users.loginPolicyTitle')"
    :confirm-loading="saving"
    @ok="save"
    @update:open="emit('update:open', $event)"
  >
    <a-form layout="vertical">
      <a-form-item :label="t('users.maxFailedAttempts')">
        <a-input-number v-model:value="form.maxFailedAttempts" :min="1" :max="50" style="width: 100%" />
      </a-form-item>
      <a-form-item :label="t('users.lockSeconds')">
        <a-input-number v-model:value="form.lockSeconds" :min="60" :max="86400" style="width: 100%" />
      </a-form-item>
      <a-form-item :label="t('users.warningPercent')">
        <a-input-number v-model:value="form.warningPercent" :min="1" :max="99" style="width: 100%" />
      </a-form-item>
    </a-form>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 登录风控策略弹窗
// =============================================================================

import { reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

const { t } = useI18n();
const saving = ref(false);
const form = reactive({
  maxFailedAttempts: 5,
  lockSeconds: 900,
  warningPercent: 60,
});

async function load(): Promise<void> {
  try {
    Object.assign(form, await http.get<typeof form>('/api/system/login-policy'));
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('users.loginPolicyLoadFailed'));
  }
}

async function save(): Promise<void> {
  saving.value = true;
  try {
    await http.put('/api/system/login-policy', form);
    message.success(t('users.loginPolicySaved'));
    emit('update:open', false);
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('users.saveFailed'));
  } finally {
    saving.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) void load();
  },
);
</script>
