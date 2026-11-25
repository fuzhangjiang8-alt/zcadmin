<template>
  <a-modal
    :open="open"
    :title="t('audit.snapshotTitle')"
    :footer="null"
    width="760px"
    @update:open="emit('update:open', $event)"
  >
    <a-select
      v-model:value="filter"
      allow-clear
      :placeholder="t('audit.filterPlaceholder')"
      style="width: 180px; margin-bottom: 12px"
      @change="() => load(1)"
    >
      <a-select-option value="all">{{ t('audit.filterAll') }}</a-select-option>
      <a-select-option value="rules">{{ t('audit.filterRules') }}</a-select-option>
      <a-select-option value="subscriptions">{{ t('audit.filterSubscriptions') }}</a-select-option>
      <a-select-option value="webhook">{{ t('audit.filterWebhook') }}</a-select-option>
      <a-select-option value="rules,subscriptions">{{ t('audit.filterRulesSubs') }}</a-select-option>
    </a-select>
    <a-input v-model:value="keyword" :placeholder="t('audit.keywordPlaceholder')" allow-clear style="width: 160px; margin-left: 8px" @press-enter="() => load(1)" />
    <a-input v-model:value="creator" :placeholder="t('audit.creatorPlaceholder')" allow-clear style="width: 140px; margin-left: 8px" @press-enter="() => load(1)" />
    <a-input v-model:value="from" type="date" style="width: 150px; margin-left: 8px" @change="() => load(1)" />
    <a-input v-model:value="to" type="date" style="width: 150px; margin-left: 8px" @change="() => load(1)" />
    <a-popconfirm :title="t('audit.cleanupConfirm')" @confirm="cleanup">
      <a-button v-perm="PERMS.AUDIT_ALERT" danger style="margin-left: 8px">{{ t('audit.cleanup') }}</a-button>
    </a-popconfirm>
    <a-button style="margin-left: 8px" :disabled="selectedIds.length !== 2" @click="compare">
      {{ t('audit.compare') }}
    </a-button>
    <a-table
      :columns="columns"
      :data-source="snapshots"
      row-key="id"
      :loading="loading"
      :pagination="{ current: page, pageSize: 10, total, onChange: load }"
      :row-selection="{
        selectedRowKeys: selectedIds,
        onChange: (keys: string[]) => (selectedIds = keys),
      }"
      size="small"
    >
      <template #bodyCell="{ column, record }">
        <template v-if="column.key === 'summary'">
          {{ record.diffSummary ? t('audit.summaryText', { rules: record.diffSummary.rules, subs: record.diffSummary.subscriptions }) : '-' }}
        </template>
        <template v-if="column.key === 'actions'">
          <a-button type="link" size="small" @click="showDiff(record)">{{ t('audit.detail') }}</a-button>
          <a-button v-perm="PERMS.AUDIT_ALERT" type="link" size="small" @click="startRollback(record)">{{ t('audit.rollback') }}</a-button>
          <a-button v-perm="PERMS.AUDIT_ALERT" type="link" size="small" @click="openLabel(record)">{{ t('audit.rename') }}</a-button>
          <a-button v-perm="PERMS.AUDIT_ALERT" type="link" size="small" @click="exportSnapshot(record)">{{ t('audit.exportSnapshot') }}</a-button>
          <a-popconfirm :title="t('audit.deleteSnapshot')" @confirm="removeSnapshot(record)">
            <a-button v-perm="PERMS.AUDIT_ALERT" type="link" danger size="small">{{ t('audit.delete') }}</a-button>
          </a-popconfirm>
        </template>
      </template>
    </a-table>

    <a-modal v-model:open="labelOpen" :title="t('audit.labelTitle')" :confirm-loading="labelSaving" @ok="saveLabel">
      <a-form layout="vertical">
        <a-form-item :label="t('audit.name')">
          <a-input v-model:value="labelForm.name" :maxlength="100" />
        </a-form-item>
        <a-form-item :label="t('audit.note')">
          <a-textarea v-model:value="labelForm.note" :rows="3" :maxlength="500" />
        </a-form-item>
      </a-form>
    </a-modal>

    <a-modal v-model:open="compareOpen" :title="t('audit.compareTitle')" :footer="null" width="720px">
      <div v-if="compareDiff">
        <p>{{ t('audit.rulesAdded') }}{{ compareDiff.rulesAdded.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.rulesChanged') }}{{ compareDiff.rulesChanged.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.rulesRemoved') }}{{ compareDiff.rulesRemoved.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.subsAdded') }}{{ compareDiff.subscriptionsAdded.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.subsRemoved') }}{{ compareDiff.subscriptionsRemoved.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.webhookChanged') }}{{ compareDiff.webhookChanged ? t('audit.yes') : t('audit.no') }}</p>
        <p>{{ t('audit.subWebhookChanged') }}{{ compareDiff.subscriptionWebhookChanged ? t('audit.yes') : t('audit.no') }}</p>
      </div>
    </a-modal>

    <a-modal v-model:open="diffOpen" :title="t('audit.diffTitle')" :footer="null" width="720px">
      <pre style="max-height: 60vh; overflow: auto">{{ JSON.stringify(diffDetail, null, 2) }}</pre>
    </a-modal>

    <a-modal v-model:open="rollbackOpen" :title="t('audit.rollbackTitle')" :confirm-loading="rollbackLoading" @ok="confirmRollback">
      <div v-if="rollbackDiff">
        <p>{{ t('audit.willAddRules') }}{{ rollbackDiff.rulesAdded.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.willChangeRules') }}{{ rollbackDiff.rulesChanged.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.willRemoveRules') }}{{ rollbackDiff.rulesRemoved.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.willAddSubs') }}{{ rollbackDiff.subscriptionsAdded.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.willRemoveSubs') }}{{ rollbackDiff.subscriptionsRemoved.join(t('audit.joinSep')) || t('audit.none') }}</p>
        <p>{{ t('audit.webhookChanged') }}{{ rollbackDiff.webhookChanged ? t('audit.yes') : t('audit.no') }}</p>
        <p>{{ t('audit.subWebhookChanged') }}{{ rollbackDiff.subscriptionWebhookChanged ? t('audit.yes') : t('audit.no') }}</p>
      </div>
    </a-modal>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 配置快照弹窗
//
// 支持：分页/筛选、命名、对比、导出、删除、清理、回滚。
// 回滚成功后 emit('changed')，由父页面刷新统计。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';
import { PERMS } from '../../permissions';
import type { AuditConfigDiff, AuditSnapshotRow } from './types';

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void; (e: 'changed'): void }>();

const { t } = useI18n();
const loading = ref(false);
const page = ref(1);
const total = ref(0);
const filter = ref<string | undefined>(undefined);
const keyword = ref('');
const creator = ref('');
const from = ref('');
const to = ref('');
const snapshots = ref<AuditSnapshotRow[]>([]);
const selectedIds = ref<string[]>([]);

const labelOpen = ref(false);
const labelSaving = ref(false);
const labelForm = reactive({ id: '', name: '', note: '' });

const compareOpen = ref(false);
const compareDiff = ref<AuditConfigDiff | null>(null);

const diffOpen = ref(false);
const diffDetail = ref<unknown>(null);

const rollbackOpen = ref(false);
const rollbackLoading = ref(false);
const rollbackBundle = ref<unknown>(null);
const rollbackDiff = ref<AuditConfigDiff | null>(null);

const columns = computed(() => [
  { title: t('audit.colName'), dataIndex: 'name', key: 'name' },
  { title: t('audit.colCreated'), dataIndex: 'createdAt', key: 'createdAt' },
  { title: t('audit.colCreator'), dataIndex: 'createdByName', key: 'createdByName' },
  { title: t('audit.colSections'), dataIndex: 'sections', key: 'sections' },
  { title: t('audit.colSummary'), key: 'summary' },
  { title: t('audit.colActions'), key: 'actions', width: 90 },
]);

async function load(target = 1): Promise<void> {
  loading.value = true;
  try {
    const params = new URLSearchParams({ page: String(target), size: '10' });
    if (filter.value) params.set('sections', filter.value);
    if (keyword.value) params.set('keyword', keyword.value);
    if (creator.value) params.set('createdBy', creator.value);
    if (from.value) params.set('from', from.value);
    if (to.value) params.set('to', to.value);
    const res = await http.get<{ data: AuditSnapshotRow[]; page: { total: number } }>(
      `/api/system/audit-config/snapshots?${params.toString()}`,
    );
    snapshots.value = res.data;
    page.value = target;
    total.value = res.page.total;
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.snapshotLoadFailed'));
  } finally {
    loading.value = false;
  }
}

function openLabel(record: AuditSnapshotRow): void {
  labelForm.id = record.id;
  labelForm.name = record.name ?? '';
  labelForm.note = record.note ?? '';
  labelOpen.value = true;
}

async function saveLabel(): Promise<void> {
  labelSaving.value = true;
  try {
    await http.put(`/api/system/audit-config/snapshots/${labelForm.id}`, {
      name: labelForm.name || null,
      note: labelForm.note || null,
    });
    message.success(t('audit.saved'));
    labelOpen.value = false;
    await load(page.value);
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.saveFailed'));
  } finally {
    labelSaving.value = false;
  }
}

async function compare(): Promise<void> {
  if (selectedIds.value.length !== 2) return;
  try {
    const [left, right] = selectedIds.value;
    compareDiff.value = await http.get<AuditConfigDiff>(
      `/api/system/audit-config/snapshots/compare?left=${left}&right=${right}`,
    );
    compareOpen.value = true;
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.compareFailed'));
  }
}

async function exportSnapshot(record: AuditSnapshotRow): Promise<void> {
  try {
    const bundle = await http.get<unknown>(`/api/system/audit-config/snapshots/${record.id}/export`);
    const blob = new Blob([JSON.stringify(bundle, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `snapshot-${record.id}.json`;
    link.click();
    URL.revokeObjectURL(url);
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.exportFailed'));
  }
}

async function cleanup(): Promise<void> {
  try {
    const count = await http.delete<number>('/api/system/audit-config/snapshots?keep=20');
    message.success(t('audit.cleaned', { n: count }));
    await load(1);
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.cleanupFailed'));
  }
}

async function removeSnapshot(record: AuditSnapshotRow): Promise<void> {
  try {
    await http.delete(`/api/system/audit-config/snapshots/${record.id}`);
    message.success(t('audit.deleted'));
    await load(page.value);
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.deleteFailed'));
  }
}

function showDiff(record: AuditSnapshotRow): void {
  diffDetail.value = record.diffDetail;
  diffOpen.value = true;
}

async function startRollback(record: AuditSnapshotRow): Promise<void> {
  try {
    rollbackDiff.value = await http.post<AuditConfigDiff>(
      '/api/system/audit-config/preview?sections=rules,subscriptions,webhook',
      record.bundle,
    );
    rollbackBundle.value = record.bundle;
    rollbackOpen.value = true;
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.previewFailed'));
  }
}

async function confirmRollback(): Promise<void> {
  rollbackLoading.value = true;
  try {
    await http.put(
      '/api/system/audit-config/import?sections=rules,subscriptions,webhook&mode=rollback',
      rollbackBundle.value,
    );
    message.success(t('audit.rolledBack'));
    rollbackOpen.value = false;
    emit('update:open', false);
    emit('changed');
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.rollbackFailed'));
  } finally {
    rollbackLoading.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) void load(1);
  },
);
</script>
