<template>
  <a-modal
    :open="open"
    :title="t('audit.rulesTitle')"
    :confirm-loading="saving"
    width="720px"
    @ok="save"
    @update:open="emit('update:open', $event)"
  >
    <a-alert
      type="info"
      show-icon
      :message="t('audit.rulesHint')"
      style="margin-bottom: 12px"
    />
    <a-table :columns="columns" :data-source="rules" :pagination="false" size="small" row-key="actionPattern">
      <template #bodyCell="{ column, record, index }">
        <template v-if="column.key === 'pattern'">
          <a-input v-model:value="record.actionPattern" :placeholder="t('audit.actionPatternPlaceholder')" />
        </template>
        <template v-else-if="column.key === 'window'">
          <a-input-number v-model:value="record.windowMinutes" :min="1" :max="60" />
        </template>
        <template v-else-if="column.key === 'threshold'">
          <a-input-number v-model:value="record.threshold" :min="1" :max="10000" />
        </template>
        <template v-else-if="column.key === 'enabled'">
          <a-switch v-model:checked="record.enabled" />
        </template>
        <template v-else-if="column.key === 'actions'">
          <a-button type="link" danger size="small" @click="rules.splice(index, 1)">{{ t('audit.delete') }}</a-button>
        </template>
      </template>
    </a-table>
    <a-button style="margin-top: 12px" @click="add">{{ t('audit.addRule') }}</a-button>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 审计告警规则管理弹窗
//
// 全量覆盖保存；前端先校验兜底规则 `*` 存在，避免后端 422 才提示。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';

interface AlertRule {
  actionPattern: string;
  windowMinutes: number;
  threshold: number;
  enabled: boolean;
}

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

const saving = ref(false);
const { t } = useI18n();
const rules = ref<AlertRule[]>([]);

const columns = computed(() => [
  { title: t('audit.colActionPattern'), dataIndex: 'actionPattern', key: 'pattern' },
  { title: t('audit.colWindow'), key: 'window', width: 130 },
  { title: t('audit.colThreshold'), key: 'threshold', width: 120 },
  { title: t('audit.colEnabled'), key: 'enabled', width: 80 },
  { title: t('audit.colActions'), key: 'actions', width: 80 },
]);

async function load(): Promise<void> {
  try {
    const rows = await http.get<AlertRule[]>('/api/system/audit-alert-rules');
    rules.value = rows.map((r) => ({ ...r }));
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.loadFailed'));
  }
}

function add(): void {
  rules.value.push({ actionPattern: '', windowMinutes: 5, threshold: 50, enabled: true });
}

async function save(): Promise<void> {
  if (!rules.value.some((r) => r.actionPattern === '*')) {
    message.error(t('audit.fallbackRequired'));
    return;
  }
  saving.value = true;
  try {
    await http.put('/api/system/audit-alert-rules', { rules: rules.value });
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
