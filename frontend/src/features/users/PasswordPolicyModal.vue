<template>
  <a-modal
    :open="open"
    :title="t('users.passwordPolicyTitle')"
    :confirm-loading="saving"
    @ok="save"
    @update:open="emit('update:open', $event)"
  >
    <a-form layout="vertical">
      <a-form-item :label="t('users.minLength')">
        <a-input-number v-model:value="form.minLength" :min="6" :max="64" style="width: 100%" />
      </a-form-item>
      <a-form-item>
        <a-checkbox v-model:checked="form.requireUpper">{{ t('users.requireUpper') }}</a-checkbox>
        <a-checkbox v-model:checked="form.requireLower">{{ t('users.requireLower') }}</a-checkbox>
        <a-checkbox v-model:checked="form.requireDigit">{{ t('users.requireDigit') }}</a-checkbox>
      </a-form-item>
      <a-form-item :label="t('users.historyCount')">
        <a-input-number v-model:value="form.historyCount" :min="0" :max="20" style="width: 100%" />
      </a-form-item>
      <a-form-item :label="t('users.expiresDays')">
        <a-input-number v-model:value="form.expiresDays" :min="0" :max="3650" style="width: 100%" />
      </a-form-item>
    </a-form>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 密码策略弹窗
//
// 打开时拉取当前策略，保存为全量覆盖；校验规则由后端统一执行。
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
  minLength: 10,
  requireUpper: true,
  requireLower: true,
  requireDigit: true,
  historyCount: 5,
  expiresDays: 0,
});

async function load(): Promise<void> {
  try {
    Object.assign(form, await http.get<typeof form>('/api/system/password-policy'));
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('users.passwordPolicyLoadFailed'));
  }
}

async function save(): Promise<void> {
  saving.value = true;
  try {
    await http.put('/api/system/password-policy', form);
    message.success(t('users.passwordPolicySaved'));
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
