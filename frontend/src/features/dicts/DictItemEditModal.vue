<template>
  <a-modal
    :open="open"
    :title="t('dicts.itemEditTitle')"
    :confirm-loading="saving"
    @ok="submit"
    @update:open="emit('update:open', $event)"
  >
    <a-form layout="vertical">
      <a-form-item :label="t('dicts.label')">
        <a-input v-model:value="form.label" :placeholder="t('dicts.labelPlaceholder')" />
      </a-form-item>
      <a-form-item :label="t('dicts.value')">
        <a-input v-model:value="form.value" :placeholder="t('dicts.valuePlaceholder')" />
      </a-form-item>
      <a-form-item :label="t('dicts.sort')">
        <a-input-number v-model:value="form.sort" :min="0" />
      </a-form-item>
    </a-form>
  </a-modal>
</template>

<script setup lang="ts">
// 字典项编辑弹窗。
//
// 独立组件：只负责 label/value/sort 的编辑与提交，保存成功后 emit('saved')
// 让父页面刷新字典项列表；code 由父页面传入（不可变）。
import { reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';

interface DictItemVo {
  id: string;
  label: string;
  value: string;
  sort: number;
}

const props = defineProps<{
  open: boolean;
  code: string;
  item: DictItemVo | null;
}>();
const emit = defineEmits<{
  (e: 'update:open', v: boolean): void;
  (e: 'saved'): void;
}>();

const { t } = useI18n();
const saving = ref(false);
const form = reactive({ label: '', value: '', sort: 0 });

watch(
  () => props.item,
  (item) => {
    if (!item) return;
    form.label = item.label;
    form.value = item.value;
    form.sort = item.sort;
  },
  { immediate: true },
);

async function submit(): Promise<void> {
  if (!props.item) return;
  if (!form.label.trim() || !form.value.trim()) {
    message.warning(t('dicts.labelValueRequired'));
    return;
  }
  saving.value = true;
  try {
    await http.put(`/api/system/dicts/${props.code}/items/${props.item.id}`, {
      label: form.label.trim(),
      value: form.value.trim(),
      sort: form.sort,
    });
    message.success(t('dicts.saved'));
    emit('update:open', false);
    emit('saved');
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('dicts.saveFailed'));
  } finally {
    saving.value = false;
  }
}
</script>
