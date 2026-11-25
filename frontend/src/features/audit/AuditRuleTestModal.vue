<template>
  <a-modal :open="open" :title="t('audit.testTitle')" :footer="null" @update:open="emit('update:open', $event)">
    <a-form layout="vertical">
      <a-form-item :label="t('audit.action')">
        <a-input v-model:value="form.action" :placeholder="t('audit.actionPlaceholder')" />
      </a-form-item>
      <a-form-item :label="t('audit.count')">
        <a-input-number v-model:value="form.count" :min="1" style="width: 100%" />
      </a-form-item>
    </a-form>
    <a-button type="primary" :loading="loading" @click="run">{{ t('audit.test') }}</a-button>
    <a-descriptions v-if="result" :column="1" size="small" bordered style="margin-top: 12px">
      <a-descriptions-item :label="t('audit.matchedPattern')">{{ result.matchedPattern }}</a-descriptions-item>
      <a-descriptions-item :label="t('audit.windowMinutes')">{{ result.windowMinutes }}</a-descriptions-item>
      <a-descriptions-item :label="t('audit.threshold')">{{ result.threshold }}</a-descriptions-item>
      <a-descriptions-item :label="t('audit.silenceMinutes')">{{ result.silenceMinutes }}</a-descriptions-item>
      <a-descriptions-item :label="t('audit.wouldAlert')">{{ result.wouldAlert ? t('audit.yes') : t('audit.no') }}</a-descriptions-item>
      <a-descriptions-item :label="t('audit.reason')">{{ result.reason }}</a-descriptions-item>
    </a-descriptions>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 审计告警规则测试弹窗
//
// 只做预览：后端按当前规则计算是否命中、是否会触发告警，不产生真实告警。
// =============================================================================

import { reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';

interface RuleTestResult {
  matchedPattern: string;
  windowMinutes: number;
  threshold: number;
  silenceMinutes: number;
  wouldAlert: boolean;
  reason: string;
}

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

const { t } = useI18n();
const loading = ref(false);
const form = reactive({ action: '', count: 60 });
const result = ref<RuleTestResult | null>(null);

async function run(): Promise<void> {
  loading.value = true;
  try {
    result.value = await http.post<RuleTestResult>(
      '/api/system/audit-alert-rules/preview',
      { ...form },
    );
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('audit.testFailed'));
  } finally {
    loading.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    result.value = null;
    form.action = '';
    form.count = 60;
  },
);
</script>
