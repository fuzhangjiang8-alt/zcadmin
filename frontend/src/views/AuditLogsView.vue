<template>
  <div class="page">
    <AuditLogTable ref="logTable" @detail="openDetail">
      <template #actions>
        <a-button v-perm="PERMS.AUDIT_ALERT" @click="openAlertConfig">{{ t('audit.alertSettings') }}</a-button>
        <a-button v-perm="PERMS.AUDIT_ALERT" @click="webhookOpen = true">Webhook</a-button>
        <a-button v-perm="PERMS.AUDIT_ALERT" @click="rulesOpen = true">{{ t('audit.rulesManage') }}</a-button>
        <a-button v-perm="PERMS.AUDIT_ALERT" @click="ruleTestOpen = true">{{ t('audit.ruleTest') }}</a-button>
        <a-button v-perm="PERMS.AUDIT_ALERT" @click="openDelivery('dead')">{{ t('audit.deadLetters') }}</a-button>
        <a-button v-perm="PERMS.AUDIT_ALERT" @click="openDelivery('deliveries')">{{ t('audit.deliveryDead') }}</a-button>
        <a-button v-perm="PERMS.AUDIT_LIST" @click="openDelivery('stats')">{{ t('audit.deliveryStats') }}</a-button>
        <a-button v-perm="PERMS.AUDIT_ALERT" @click="snapshotsOpen = true">{{ t('audit.snapshots') }}</a-button>
        <a-button v-perm="PERMS.AUDIT_LIST" @click="exportConfig">{{ t('audit.exportConfig') }}</a-button>
        <a-upload v-perm="PERMS.AUDIT_ALERT" :before-upload="importConfig" accept=".json" :show-upload-list="false">
          <a-button>{{ t('audit.importConfig') }}</a-button>
        </a-upload>
        <a-button @click="openSub('config')">{{ t('audit.subscription') }}</a-button>
        <a-button @click="openSub('alerts')">{{ t('audit.myAlerts') }}</a-button>
      </template>
    </AuditLogTable>

    <AuditSubscriptionsModal v-model:open="subOpen" :initial-tab="subTab" @changed="refresh" />

    <AuditSnapshotsModal v-model:open="snapshotsOpen" @changed="refresh" />
    <AuditDeliveryModal v-model:open="deliveryOpen" :initial-tab="deliveryTab" />
    <AuditWebhookModal v-model:open="webhookOpen" />
    <AuditAlertRulesModal v-model:open="rulesOpen" />
    <AuditRuleTestModal v-model:open="ruleTestOpen" />

    <a-modal v-model:open="importOpen" :title="t('audit.importDiffTitle')" :confirm-loading="importing" @ok="confirmImport">
      <a-checkbox-group
        v-model:value="importSections"
        :options="sectionOptions"
        style="margin-bottom: 12px"
        @change="refreshPreview"
      />
      <div v-if="importDiff">
        <p>{{ t('audit.rulesAdded') }}{{ importDiff.rulesAdded.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.rulesChanged') }}{{ importDiff.rulesChanged.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.rulesRemoved') }}{{ importDiff.rulesRemoved.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.subsAdded') }}{{ importDiff.subscriptionsAdded.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.subsRemoved') }}{{ importDiff.subscriptionsRemoved.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.webhookChanged') }}{{ importDiff.webhookChanged ? t('audit.yes') : t('audit.no') }}</p>
        <p>{{ t('audit.subWebhookChanged') }}{{ importDiff.subscriptionWebhookChanged ? t('audit.yes') : t('audit.no') }}</p>
      </div>
    </a-modal>

    <a-modal
      v-model:open="configOpen"
      :title="t('audit.auditAlertSettings')"
      :confirm-loading="configSaving"
      @ok="saveAlertConfig"
    >
      <a-form layout="vertical">
        <a-form-item :label="t('audit.statWindow')">
          <a-input-number v-model:value="configForm.windowMinutes" :min="1" :max="60" />
        </a-form-item>
        <a-form-item :label="t('audit.triggerThreshold')">
          <a-input-number v-model:value="configForm.threshold" :min="1" :max="10000" />
        </a-form-item>
      </a-form>
    </a-modal>

    <a-drawer v-model:open="detailOpen" :title="t('audit.auditDetail')" width="520">
      <template v-if="currentLog">
        <a-descriptions :column="1" bordered size="small">
          <a-descriptions-item :label="t('audit.dAction')">{{ currentLog.action }}</a-descriptions-item>
          <a-descriptions-item :label="t('audit.dResource')">
            {{ currentLog.resourceType }} / {{ currentLog.resourceId ?? '—' }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('audit.dResult')">{{ currentLog.result }}</a-descriptions-item>
          <a-descriptions-item :label="t('audit.dTime')">{{ currentLog.createdAt }}</a-descriptions-item>
        </a-descriptions>
        <div class="json-title">{{ t('audit.changeContent') }}</div>
        <pre class="json">{{ prettyDetail }}</pre>
      </template>
    </a-drawer>
  </div>
</template>

<script setup lang="ts">
// =============================================================================
// 审计日志页
//
// 列表/筛选/统计/告警横幅在 `AuditLogTable`；本文件只做弹窗编排：
// 告警设置、Webhook、规则、规则测试、投递、快照、订阅、配置导入导出、详情抽屉。
// =============================================================================

import { computed, reactive, ref } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../api/client';
import { PERMS } from '../permissions';
import AuditAlertRulesModal from '../features/audit/AuditAlertRulesModal.vue';
import AuditDeliveryModal from '../features/audit/AuditDeliveryModal.vue';
import AuditLogTable from '../components/AuditLogTable.vue';
import AuditRuleTestModal from '../features/audit/AuditRuleTestModal.vue';
import AuditSnapshotsModal from '../features/audit/AuditSnapshotsModal.vue';
import AuditSubscriptionsModal from '../features/audit/AuditSubscriptionsModal.vue';
import AuditWebhookModal from '../features/audit/AuditWebhookModal.vue';
import type { AuditConfigDiff, AuditLogVo } from '../features/audit/types';

const logTable = ref<InstanceType<typeof AuditLogTable> | null>(null);
const detailOpen = ref(false);
const currentLog = ref<AuditLogVo | null>(null);
const prettyDetail = computed(() =>
  currentLog.value?.detail ? JSON.stringify(currentLog.value.detail, null, 2) : '—',
);

function openDetail(record: AuditLogVo): void {
  currentLog.value = record;
  detailOpen.value = true;
}

function refresh(): void {
  void logTable.value?.reload();
}

// ---- 告警设置 ----
const configOpen = ref(false);
const configSaving = ref(false);
const configForm = reactive({ windowMinutes: 5, threshold: 50 });

async function openAlertConfig(): Promise<void> {
  const cfg = await http.get<{ windowMinutes: number; threshold: number }>(
    '/api/system/audit-alert-config',
  );
  configForm.windowMinutes = cfg.windowMinutes;
  configForm.threshold = cfg.threshold;
  configOpen.value = true;
}

async function saveAlertConfig(): Promise<void> {
  configSaving.value = true;
  try {
    await http.put('/api/system/audit-alert-config', { ...configForm });
    message.success(t('audit.saved'));
    configOpen.value = false;
    await logTable.value?.loadStats();
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.saveFailed'));
  } finally {
    configSaving.value = false;
  }
}

// ---- 订阅 ----
const webhookOpen = ref(false);
const rulesOpen = ref(false);
const ruleTestOpen = ref(false);

const subOpen = ref(false);
const subTab = ref<'alerts' | 'config'>('alerts');

function openSub(tab: 'alerts' | 'config'): void {
  subTab.value = tab;
  subOpen.value = true;
}

// ---- 投递 ----
const deliveryOpen = ref(false);
const deliveryTab = ref<'stats' | 'deliveries' | 'dead'>('stats');

function openDelivery(tab: 'stats' | 'deliveries' | 'dead'): void {
  deliveryTab.value = tab;
  deliveryOpen.value = true;
}

// ---- 配置导入导出 ----
const snapshotsOpen = ref(false);
const { t } = useI18n();
const importOpen = ref(false);
const importing = ref(false);
const importDiff = ref<AuditConfigDiff | null>(null);
const importBundle = ref<unknown>(null);
const importSections = ref<string[]>(['rules', 'subscriptions', 'webhook']);
const sectionOptions = [
  { label: t('audit.sectionRules'), value: 'rules' },
  { label: t('audit.sectionSubs'), value: 'subscriptions' },
  { label: 'Webhook', value: 'webhook' },
];

async function exportConfig(): Promise<void> {
  const cfg = await http.get<unknown>('/api/system/audit-config/export');
  const blob = new Blob([JSON.stringify(cfg, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = 'audit-config.json';
  link.click();
  URL.revokeObjectURL(url);
}

async function importConfig(file: File): Promise<boolean> {
  try {
    const cfg = JSON.parse(await file.text());
    importDiff.value = await http.post<AuditConfigDiff>(
      `/api/system/audit-config/preview?sections=${importSections.value.join(',')}`,
      cfg,
    );
    importBundle.value = cfg;
    importOpen.value = true;
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.importParseFailed'));
  }
  return false;
}

async function refreshPreview(): Promise<void> {
  if (!importBundle.value) return;
  importDiff.value = await http.post<AuditConfigDiff>(
    `/api/system/audit-config/preview?sections=${importSections.value.join(',')}`,
    importBundle.value,
  );
}

async function confirmImport(): Promise<void> {
  importing.value = true;
  try {
    await http.put(
      `/api/system/audit-config/import?sections=${importSections.value.join(',')}`,
      importBundle.value,
    );
    message.success(t('audit.importDone'));
    importOpen.value = false;
    refresh();
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.importFailed'));
  } finally {
    importing.value = false;
  }
}
</script>

<style scoped>
.page {
  padding: 0;
}

.muted {
  color: #94a3b8;
}

.json-title {
  margin: 20px 0 8px;
  font-weight: 600;
}

.json {
  background: #0f172a;
  color: #e2e8f0;
  border-radius: 6px;
  padding: 12px;
  font-size: 12px;
  line-height: 1.6;
  max-height: 420px;
  overflow: auto;
}
</style>
