<template>
  <a-card :bordered="false" :title="t('audit.logTitle')">
    <template #extra>
      <a-space>
        <a-select
          v-model:value="resourceType"
          :options="resourceOptions"
          allow-clear
          :placeholder="t('audit.resourcePlaceholder')"
          style="width: 160px"
          @change="reload"
        />
        <a-select
          v-model:value="action"
          :options="actionOptions"
          show-search
          allow-clear
          :placeholder="t('audit.actionPlaceholder')"
          style="width: 260px"
          @change="reload"
        />
        <a-range-picker
          v-model:value="timeRange"
          show-time
          allow-clear
          style="width: 360px"
          @change="reload"
        />
        <a-input v-model:value="ipFilter" allow-clear :placeholder="t('audit.ipPlaceholder')" style="width: 140px" @press-enter="reload" />
        <a-input v-model:value="uaFilter" allow-clear placeholder="User-Agent" style="width: 180px" @press-enter="reload" />
        <slot name="actions" />
      </a-space>
    </template>

    <div v-if="stats" class="stats">
      <a-statistic :title="t('audit.statTotal')" :value="stats.total" />
      <template v-if="alertStats">
        <a-statistic :title="t('audit.statUnacked')" :value="alertStats.unacked" />
        <a-statistic :title="t('audit.statAcked')" :value="alertStats.acknowledged" />
        <a-statistic :title="t('audit.statAvgAck')" :value="alertStats.avgAckMinutes.toFixed(1)" />
        <a-statistic :title="t('audit.statNotifyFailed')" :value="alertStats.notifyFailed" />
      </template>
      <template v-if="deliveryStats">
        <a-statistic :title="t('audit.statDeliveryTotal')" :value="deliveryStats.total" />
        <a-statistic :title="t('audit.statPending')" :value="deliveryStats.pending" />
        <a-statistic :title="t('audit.statDead')" :value="deliveryStats.dead" />
      </template>
      <div class="stat-actions">
        <a-tag v-for="a in stats.byAction.slice(0, 6)" :key="a.action">
          {{ a.action }} · {{ a.count }}
        </a-tag>
      </div>
    </div>

    <a-alert v-for="al in alerts" :key="al.id" type="warning" show-icon style="margin-bottom: 8px">
      <template #message>
        {{ t('audit.alertBanner', { action: al.action, user: al.username ? t('audit.alertUserSuffix', { name: al.username }) : '', count: al.count }) }}
      </template>
      <template #action>
        <a-button v-perm="PERMS.AUDIT_ALERT" size="small" @click="ackAlert(al)">{{ t('audit.handle') }}</a-button>
      </template>
    </a-alert>

    <a-alert
      type="info"
      show-icon
      :message="t('audit.logHint')"
      style="margin-bottom: 16px"
    />

    <a-table
      :columns="columns"
      :data-source="logs"
      :loading="loading"
      row-key="id"
      :pagination="{
        current: page,
        pageSize: size,
        total,
        showSizeChanger: true,
        showTotal: (total: number) => t('audit.totalCount', { n: total }),
      }"
      @change="onTableChange"
    >
      <template #bodyCell="{ column, record }">
        <template v-if="column.key === 'result'">
          <a-tag :color="record.result === 'success' ? 'green' : 'red'">{{ record.result }}</a-tag>
        </template>
        <template v-else-if="column.key === 'detail'">
          <span class="detail">{{ compact(record.detail) }}</span>
          <a class="detail-link" @click="emit('detail', record)">{{ t('audit.detail') }}</a>
        </template>
      </template>
    </a-table>
  </a-card>
</template>

<script setup lang="ts">
// =============================================================================
// 审计日志表格（筛选 + 统计 + 告警横幅 + 列表）
//
// 只负责「读」；打开的弹窗由父页面通过 #actions 插槽提供。
// 对外暴露 reload()，供导入/回滚后刷新。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { TablePaginationConfig } from 'ant-design-vue';
import type { Dayjs } from 'dayjs';
import { ApiError, http } from '../api/client';
import { PERMS } from '../permissions';
import type { AuditAlert, AuditLogVo, AuditStats, AlertStats, DeliveryStats, PageResult } from '../features/audit/types';

const emit = defineEmits<{ (e: 'detail', record: AuditLogVo): void }>();

const { t } = useI18n();
const queryClient = useQueryClient();
const page = ref(1);
const size = ref(20);
const action = ref<string | undefined>(undefined);
const resourceType = ref<string | undefined>(undefined);
const ipFilter = ref('');
const uaFilter = ref('');
const timeRange = ref<[Dayjs, Dayjs] | null>(null);

// 只有点「查询」或选择筛选项时才把当前条件写入 applied，
// 避免 IP/UA 输入过程中每个字符都发请求。
const applied = ref({
  action: undefined as string | undefined,
  resourceType: undefined as string | undefined,
  ip: '',
  ua: '',
  from: '',
  to: '',
});

const query = useQuery({
  queryKey: computed(() => ['audit-logs', page.value, size.value, applied.value] as const),
  queryFn: () => {
    const params = new URLSearchParams({ page: String(page.value), size: String(size.value) });
    if (applied.value.action) params.set('action', applied.value.action);
    if (applied.value.ip) params.set('ip', applied.value.ip);
    if (applied.value.ua) params.set('userAgent', applied.value.ua);
    if (applied.value.from) params.set('from', applied.value.from);
    if (applied.value.to) params.set('to', applied.value.to);
    if (applied.value.resourceType) params.set('resourceType', applied.value.resourceType);
    return http.get<PageResult<AuditLogVo>>(`/api/system/audit-logs?${params.toString()}`);
  },
  placeholderData: keepPreviousData,
});

const logs = computed(() => query.data.value?.data ?? []);
const total = computed(() => query.data.value?.page.total ?? 0);
const loading = computed(() => query.isFetching.value);

watch(query.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('audit.loadFailed'));
});

const actionsQuery = useQuery({
  queryKey: ['audit-actions'],
  queryFn: () => http.get<string[]>('/api/system/audit-logs/actions'),
});
const actionOptions = computed(() =>
  (actionsQuery.data.value ?? []).map((item) => ({ label: item, value: item })),
);

const statsQuery = useQuery({
  queryKey: ['audit-stats'],
  queryFn: async () => {
    const [audit, alert, delivery] = await Promise.all([
      http.get<AuditStats>('/api/system/audit-logs/stats?days=7'),
      http.get<AlertStats>('/api/system/audit-alerts/stats?days=30'),
      http.get<DeliveryStats>('/api/system/audit-delivery-stats?days=30'),
    ]);
    return { audit, alert, delivery };
  },
});
const stats = computed(() => statsQuery.data.value?.audit ?? null);
const alertStats = computed(() => statsQuery.data.value?.alert ?? null);
const deliveryStats = computed(() => statsQuery.data.value?.delivery ?? null);

const alertsQuery = useQuery({
  queryKey: ['audit-alerts-open'],
  queryFn: () =>
    http.get<{ data: AuditAlert[] }>('/api/system/audit-alerts?page=1&size=3&unacked=true'),
});
const alerts = computed(() => alertsQuery.data.value?.data ?? []);

const resourceOptions = computed(() => [
  { label: t('audit.resourceCustomer'), value: 'customer' },
  { label: t('audit.resourceContact'), value: 'contact' },
  { label: t('audit.resourceOpportunity'), value: 'opportunity' },
  { label: t('audit.resourceContract'), value: 'contract' },
  { label: t('audit.resourcePayment'), value: 'payment' },
  { label: t('audit.resourceUser'), value: 'user' },
  { label: t('audit.resourceRole'), value: 'role' },
  { label: t('audit.resourceMenu'), value: 'menu' },
  { label: t('audit.resourceDept'), value: 'dept' },
  { label: t('audit.resourcePost'), value: 'post' },
]);

const columns = computed(() => [
  { title: t('audit.colTime'), dataIndex: 'createdAt', key: 'createdAt', width: 190 },
  { title: t('audit.colAction'), dataIndex: 'action', key: 'action', width: 220 },
  { title: t('audit.colResource'), dataIndex: 'resourceType', key: 'resourceType', width: 130 },
  { title: t('audit.colResourceId'), dataIndex: 'resourceId', key: 'resourceId', width: 220 },
  { title: t('audit.colIp'), dataIndex: 'ip', key: 'ip', width: 140 },
  { title: t('audit.colUa'), dataIndex: 'userAgent', key: 'userAgent', width: 220 },
  { title: t('audit.colResult'), dataIndex: 'result', key: 'result', width: 90 },
  { title: t('audit.colSummary'), dataIndex: 'detail', key: 'detail' },
]);

function compact(detail: Record<string, unknown> | null): string {
  if (!detail) return '-';
  const text = JSON.stringify(detail);
  return text.length > 80 ? `${text.slice(0, 80)}…` : text;
}

function reload(): void {
  applied.value = {
    action: action.value,
    resourceType: resourceType.value,
    ip: ipFilter.value.trim(),
    ua: uaFilter.value.trim(),
    from: timeRange.value ? timeRange.value[0].toISOString() : '',
    to: timeRange.value ? timeRange.value[1].toISOString() : '',
  };
}

function loadStats(): void {
  void statsQuery.refetch();
}

function onTableChange(pagination: TablePaginationConfig): void {
  page.value = pagination.current ?? 1;
  size.value = pagination.pageSize ?? 20;
}

const ackMutation = useMutation({
  mutationFn: (id: string) => http.post(`/api/system/audit-alerts/${id}/ack`),
  onSuccess: () => {
    message.success(t('audit.handled'));
    void queryClient.invalidateQueries({ queryKey: ['audit-alerts-open'] });
    void queryClient.invalidateQueries({ queryKey: ['audit-stats'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('audit.handleFailed'));
  },
});

function ackAlert(alert: AuditAlert): void {
  ackMutation.mutate(alert.id);
}

defineExpose({ reload, loadStats });
</script>

<style scoped>
.stats {
  display: flex;
  flex-wrap: wrap;
  gap: 32px;
  margin-bottom: 16px;
}

.stat-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
}

.detail {
  color: #64748b;
}

.detail-link {
  margin-left: 8px;
}
</style>
