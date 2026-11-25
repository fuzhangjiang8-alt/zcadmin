<template>
  <a-modal
    :open="open"
    :title="t('password.title')"
    :confirm-loading="saving"
    :closable="!mustChange"
    :mask-closable="false"
    :cancel-button-props="{ style: mustChange ? 'display:none' : '' }"
    @ok="submit"
    @update:open="emit('update:open', $event)"
  >
    <a-form layout="vertical">
      <a-form-item :label="t('password.old')">
        <a-input-password v-model:value="form.oldPassword" />
      </a-form-item>
      <a-form-item :label="t('password.next')">
        <a-input-password v-model:value="form.newPassword" />
      </a-form-item>
      <a-form-item :label="t('password.confirm')">
        <a-input-password v-model:value="form.confirm" />
      </a-form-item>
      <a-form-item>
        <a-checkbox v-model:checked="form.revokeTokens">{{ t('password.revokeTokens') }}</a-checkbox>
      </a-form-item>
    </a-form>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 修改密码弹窗
//
// 首次登录强制改密时不可关闭；改密成功后若仍处于 mustChange 状态，
// 清空会话并回登录页（后端会吊销全部会话）。
// =============================================================================

import { reactive, ref } from 'vue';
import { useRouter } from 'vue-router';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { http } from '../../api/client';
import { clearSession } from '../../stores/session';

const props = defineProps<{ open: boolean; mustChange: boolean }>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

const router = useRouter();
const { t } = useI18n();
const saving = ref(false);
const form = reactive({ oldPassword: '', newPassword: '', confirm: '', revokeTokens: true });

async function submit(): Promise<void> {
  if (!form.oldPassword || !form.newPassword) {
    message.warning(t('password.fillBoth'));
    return;
  }
  if (form.newPassword !== form.confirm) {
    message.warning(t('password.mismatch'));
    return;
  }
  saving.value = true;
  try {
    await http.post('/api/auth/change-password', {
      oldPassword: form.oldPassword,
      newPassword: form.newPassword,
      revokeTokens: form.revokeTokens,
    });
    if (props.mustChange) {
      message.success(t('password.changedRelogin'));
      clearSession();
      await router.replace('/login');
      return;
    }
    message.success(t('password.changed'));
    emit('update:open', false);
    form.oldPassword = '';
    form.newPassword = '';
    form.confirm = '';
  } catch (error) {
    message.error(error instanceof Error ? error.message : t('password.changeFailed'));
  } finally {
    saving.value = false;
  }
}
</script>
