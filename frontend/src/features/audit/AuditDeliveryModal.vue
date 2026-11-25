<template>
  <a-modal
    :open="open"
    :title="t('audit.deliveryTitle')"
    :footer="null"
    width="760px"
    @update:open="emit('update:open', $event)"
  >
    <a-tabs v-model:activeKey="tab" @change="loadTab">
      <a-tab-pane key="stats" :tab="t('audit.deliveryStats')">
        <a-table
          :columns="userColumns"
          :data-source="stats?.byUser ?? []"
          row-key="username"
          :pagination="false"
          size="small"
        />
        <a-divider orientation="left">{{ t('audit.deadLetterReasons') }}</a-divider>
        <a-table
          :columns="errorColumns"
          :data-source="stats?.byError ?? []"
          row-key="error"
          :pagination="false"
          size="small"
        />
      </a-tab-pane>

      <a-tab-pane key="deliveries" :tab="t('audit.subDeadLetters')">
        <a-button
          v-perm="PERMS.AUDIT_ALERT"
          type="primary"
          danger
          style="margin-bottom: 12px"
          @click="retryAll"
        >
          {{ t('audit.retryAll') }}
        </a-button>
        <a-table
          :columns="deliveryColumns"
          :data-source="deliveries"
          row-key="id"
          :loading="loading"
          :pagination="false"
          size="small"
        >
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'actions'">
              <a-button v-perm="PERMS.AUDIT_ALERT" type="link" size="small" @click="retryDelivery(record)">
                {{ t('audit.retry') }}
              </a-button>
            </template>
          </template>
        </a-table>
        <div v-if="deliveries.length === 0" class="muted" style="margin-top: 12px">{{ t('audit.noSubDead') }}</div>
      </a-tab-pane>

      <a-tab-pane key="dead" :tab="t('audit.webhookDead')">
        <a-table :columns="deadColumns" :data-source="deadRows" row-key="id" :pagination="false" size="small">
          <template #bodyCell="{ column, record }">
            <template v-if="column.key === 'actions'">
              <a-button v-perm="PERMS.AUDIT_ALERT" type="link" size="small" :loading="loading" @click="retryDead(record)">
                {{ t('audit.retry') }}
              </a-button>
            </template>
          </template>
        </a-table>
        <div v-if="deadRows.length === 0" class="muted" style="margin-top: 12px">{{ t('audit.noDead') }}</div>
      </a-tab-pane>
    </a-tabs>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 审计投递弹窗（统计 / 订阅死信 / Webhook 死信）
//
// 三个视图共用一个弹窗，用 tab 切换；每个 tab 打开时才发请求。
// 重投成功后重新拉取当前 tab 的数据，避免本地状态与后端不一致。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';
import { PERMS } from '../../permissions';

/** 与后端 `/audit-delivery-stats` 对齐的最小子集。 */
interface DeliveryStats {
  byUser: { username: string; total: number; sent: number; dead: number }[];
  byError: { error: string; count: number }[];
}

interface DeadAlert {
  id: string;
  action: string;
  count: number;
  notifyFailedAt: string | null;
}

const props = defineProps<{
  open: boolean;
  initialTab: 'stats' | 'deliveries' | 'dead';
}>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

const tab = ref<'stats' | 'deliveries' | 'dead'>('stats');
const { t } = useI18n();
const loading = ref(false);
const stats = ref<DeliveryStats | null>(null);
const deliveries = ref<{ id: string; action: string; url: string; attempts: number }[]>([]);
const deadRows = ref<DeadAlert[]>([]);

const userColumns = computed(() => [
  { title: t('audit.colUser'), dataIndex: 'username', key: 'username' },
  { title: t('audit.colTotal'), dataIndex: 'total', key: 'total', width: 100 },
  { title: t('audit.colSent'), dataIndex: 'sent', key: 'sent', width: 100 },
  { title: t('audit.colDead'), dataIndex: 'dead', key: 'dead', width: 100 },
]);
const errorColumns = computed(() => [
  { title: t('audit.colReason'), dataIndex: 'error', key: 'error' },
  { title: t('audit.colCount'), dataIndex: 'count', key: 'count', width: 100 },
]);
const deliveryColumns = computed(() => [
  { title: t('audit.colAction'), dataIndex: 'action', key: 'action' },
  { title: t('audit.colUrl'), dataIndex: 'url', key: 'url', ellipsis: true },
  { title: t('audit.colAttempts'), dataIndex: 'attempts', key: 'attempts', width: 80 },
  { title: t('audit.colActions'), key: 'actions', width: 90 },
]);
const deadColumns = computed(() => [
  { title: t('audit.colAction'), dataIndex: 'action', key: 'action' },
  { title: t('audit.colCount'), dataIndex: 'count', key: 'count', width: 90 },
  { title: t('audit.colActions'), key: 'actions', width: 90 },
]);

async function loadStats(): Promise<void> {
  loading.value = true;
  try {
    stats.value = await http.get<DeliveryStats>('/api/system/audit-delivery-stats?days=30');
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.loadFailed'));
  } finally {
    loading.value = false;
  }
}

async function loadDeliveries(): Promise<void> {
  loading.value = true;
  try {
    const page = await http.get<{ data: { id: string; action: string; url: string; attempts: number }[] }>(
      '/api/system/audit-deliveries?status=dead&page=1&size=100',
    );
    deliveries.value = page.data;
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.loadFailed'));
  } finally {
    loading.value = false;
  }
}

async function loadDead(): Promise<void> {
  loading.value = true;
  try {
    const page = await http.get<{ data: DeadAlert[] }>(
      '/api/system/audit-alerts?page=1&size=100&unacked=true',
    );
    deadRows.value = page.data.filter((a) => a.notifyFailedAt);
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.loadFailed'));
  } finally {
    loading.value = false;
  }
}

function loadTab(): void {
  if (tab.value === 'stats') void loadStats();
  else if (tab.value === 'deliveries') void loadDeliveries();
  else void loadDead();
}

async function retryAll(): Promise<void> {
  try {
    const count = await http.post<number>('/api/system/audit-deliveries/retry-dead');
    message.success(t('audit.resetCount', { n: count }));
    await loadDeliveries();
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.batchRetryFailed'));
  }
}

async function retryDelivery(record: { id: string }): Promise<void> {
  try {
    await http.post(`/api/system/audit-deliveries/${record.id}/retry`);
    message.success(t('audit.resetPending'));
    await loadDeliveries();
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.retryFailed'));
  }
}

async function retryDead(alert: DeadAlert): Promise<void> {
  loading.value = true;
  try {
    await http.post(`/api/system/audit-alerts/${alert.id}/retry`);
    message.success(t('audit.resetPending'));
    await loadDead();
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.retryFailed'));
  } finally {
    loading.value = false;
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
