<template>
  <a-modal
    :open="open"
    :title="t('audit.subsTitle')"
    :footer="null"
    width="720px"
    @update:open="emit('update:open', $event)"
  >
    <a-tabs v-model:activeKey="tab" @change="loadTab">
      <a-tab-pane key="alerts" :tab="t('audit.myAlerts')">
        <a-table
          :columns="alertColumns"
          :data-source="alerts"
          row-key="id"
          :loading="loading"
          :pagination="false"
          size="small"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'actions'">
              <a-button type="link" size="small" @click="ack(record)">{{ t('audit.handle') }}</a-button>
            </template>
          </template>
        </a-table>
        <div v-if="alerts.length === 0" class="muted" style="margin-top: 12px">{{ t('audit.noAlerts') }}</div>
      </a-tab-pane>

      <a-tab-pane key="config" :tab="t('audit.config')">
        <a-form layout="vertical">
          <a-form-item :label="t('audit.actionPatternLabel')">
            <a-textarea v-model:value="patterns" :rows="4" placeholder="auth.login.failed, system.import" />
          </a-form-item>
          <a-form-item :label="t('audit.webhookUrl')">
            <a-input v-model:value="url" placeholder="https://example.com/subscription-hook" />
          </a-form-item>
          <a-form-item :label="t('audit.secretLabel')">
            <a-input-password v-model:value="secret" />
          </a-form-item>
          <a-form-item :label="t('audit.payloadTemplateDefault')">
            <a-textarea v-model:value="template" :rows="3" placeholder='{"text":"{{action}} x{{count}}"}' />
          </a-form-item>
          <a-form-item :label="t('audit.enableWebhook')">
            <a-switch v-model:checked="enabled" />
          </a-form-item>
        </a-form>
        <a-button type="primary" :loading="saving" @click="save">{{ t('audit.save') }}</a-button>
      </a-tab-pane>
    </a-tabs>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 告警订阅弹窗（订阅配置 + 我的订阅告警）
// =============================================================================

import { computed, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';
import type { AuditAlert } from './types';

const props = defineProps<{
  open: boolean;
  initialTab: 'alerts' | 'config';
}>();
const emit = defineEmits<{
  (e: 'update:open', v: boolean): void;
  (e: 'changed'): void;
}>();

const tab = ref<'alerts' | 'config'>('alerts');
const loading = ref(false);
const { t } = useI18n();
const saving = ref(false);
const alerts = ref<AuditAlert[]>([]);
const patterns = ref('');
const url = ref('');
const secret = ref('');
const enabled = ref(false);
const template = ref('');

const alertColumns = computed(() => [
  { title: t('audit.colAction'), dataIndex: 'action', key: 'action' },
  { title: t('audit.colCount'), dataIndex: 'count', key: 'count', width: 90 },
  { title: t('audit.colWindowStart'), dataIndex: 'windowStart', key: 'windowStart', width: 190 },
  { title: t('audit.colActions'), key: 'actions', width: 90 },
]);

async function loadAlerts(): Promise<void> {
  loading.value = true;
  try {
    const page = await http.get<{ data: AuditAlert[] }>(
      '/api/system/audit-alerts/subscribed?page=1&size=50',
    );
    alerts.value = page.data;
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.loadFailed'));
  } finally {
    loading.value = false;
  }
}

async function loadConfig(): Promise<void> {
  loading.value = true;
  try {
    const rows = await http.get<{ actionPattern: string }[]>('/api/system/audit-alert-subscriptions');
    patterns.value = rows.map((r) => r.actionPattern).join(', ');
    const cfg = await http.get<{ url: string; enabled: boolean; payloadTemplate: string | null }>(
      '/api/system/audit-alert-subscription-config',
    );
    url.value = cfg.url;
    enabled.value = cfg.enabled;
    secret.value = '';
    template.value = cfg.payloadTemplate ?? '';
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.loadFailed'));
  } finally {
    loading.value = false;
  }
}

function loadTab(): void {
  if (tab.value === 'alerts') void loadAlerts();
  else void loadConfig();
}

async function ack(alert: AuditAlert): Promise<void> {
  try {
    await http.post(`/api/system/audit-alerts/${alert.id}/ack`);
    message.success(t('audit.handled'));
    await loadAlerts();
    emit('changed');
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.handleFailed'));
  }
}

async function save(): Promise<void> {
  saving.value = true;
  try {
    const list = patterns.value.split(',').map((s) => s.trim()).filter(Boolean);
    await http.put('/api/system/audit-alert-subscriptions', { patterns: list });
    await http.put('/api/system/audit-alert-subscription-config', {
      url: url.value,
      enabled: enabled.value,
      secret: secret.value ? secret.value : null,
      payloadTemplate: template.value ? template.value : null,
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
    if (!open) return;
    tab.value = props.initialTab;
    loadTab();
  },
);
</script>
