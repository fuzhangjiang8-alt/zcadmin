<template>
  <a-modal
    :open="open"
    :title="t('audit.webhookTitle')"
    :confirm-loading="saving"
    @ok="save"
    @update:open="emit('update:open', $event)"
  >
    <a-form layout="vertical">
      <a-form-item :label="t('audit.address')">
        <a-input v-model:value="form.url" placeholder="https://example.com/hooks/audit" />
      </a-form-item>
      <a-form-item :label="t('audit.secretLabel')">
        <a-input-password v-model:value="form.secret" />
      </a-form-item>
      <a-form-item :label="t('audit.enabled')">
        <a-switch v-model:checked="form.enabled" />
      </a-form-item>
      <a-form-item :label="t('audit.payloadTemplate')">
        <a-textarea v-model:value="form.payloadTemplate" :rows="4" />
      </a-form-item>
    </a-form>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 审计 Webhook 配置弹窗
//
// 密钥不回显：留空表示保持原密钥不变，只有填写时才覆盖。
// =============================================================================

import { reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';

interface WebhookConfig {
  url: string;
  enabled: boolean;
  payloadTemplate: string | null;
}

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

const { t } = useI18n();
const saving = ref(false);
const form = reactive({ url: '', secret: '', enabled: false, payloadTemplate: '' });

async function load(): Promise<void> {
  try {
    const cfg = await http.get<WebhookConfig>('/api/system/audit-webhook');
    form.url = cfg.url;
    form.enabled = cfg.enabled;
    form.secret = '';
    form.payloadTemplate = cfg.payloadTemplate ?? '';
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.loadFailed'));
  }
}

async function save(): Promise<void> {
  saving.value = true;
  try {
    await http.put('/api/system/audit-webhook', {
      url: form.url,
      enabled: form.enabled,
      secret: form.secret ? form.secret : null,
      payloadTemplate: form.payloadTemplate ? form.payloadTemplate : null,
    });
    message.success(t('audit.saved'));
    emit('update:open', false);
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.saveFailed'));
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
