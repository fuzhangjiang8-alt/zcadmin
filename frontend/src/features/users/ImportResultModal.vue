<template>
  <a-modal
    :open="open"
    :title="t('users.importTitle')"
    :footer="null"
    @update:open="emit('update:open', $event)"
  >
    <a-button
      v-if="result && result.errors.length > 0"
      v-perm="PERMS.USER_ADD"
      size="small"
      style="margin-bottom: 8px"
      @click="exportErrors"
    >
      {{ t('users.exportErrors') }}
    </a-button>
    <a-descriptions :column="3" size="small">
      <a-descriptions-item :label="t('users.totalRows')">{{ result?.total ?? 0 }}</a-descriptions-item>
      <a-descriptions-item :label="t('users.success')">{{ result?.success ?? 0 }}</a-descriptions-item>
      <a-descriptions-item :label="t('users.failed')">{{ result?.failed ?? 0 }}</a-descriptions-item>
    </a-descriptions>
    <a-table
      v-if="result && result.errors.length > 0"
      size="small"
      style="margin-top: 12px"
      :pagination="false"
      :data-source="result.errors"
      :columns="[
        { title: t('users.rowNo'), dataIndex: 'row', key: 'row', width: 80 },
        { title: t('users.reason'), dataIndex: 'message', key: 'message' },
      ]"
      row-key="row"
    />
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 用户导入结果弹窗
//
// 错误行可导出为 xlsx（服务端生成，前端只负责触发下载）。
// =============================================================================

import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { PERMS } from '../../permissions';
import type { ImportResult } from './types';

const { t } = useI18n();
const props = defineProps<{ open: boolean; result: ImportResult | null }>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

async function exportErrors(): Promise<void> {
  if (!props.result) return;
  try {
    const response = await fetch('/api/system/users/import-errors', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      credentials: 'include',
      body: JSON.stringify({ errors: props.result.errors }),
    });
    if (!response.ok) throw new Error(t('users.exportFailed'));
    const blob = await response.blob();
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = 'user-import-errors.xlsx';
    link.click();
    URL.revokeObjectURL(url);
  } catch (error) {
    message.error(error instanceof Error ? error.message : t('users.exportFailed'));
  }
}
</script>
