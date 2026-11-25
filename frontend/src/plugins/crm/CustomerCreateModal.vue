<template>
  <a-modal
    :open="open"
    :title="t('crm.createCustomer')"
    :confirm-loading="submitting"
    :ok-text="t('crm.create')"
    :cancel-text="t('crm.cancel')"
    @ok="submit"
    @update:open="emit('update:open', $event)"
  >
    <a-form ref="formRef" :model="form" :rules="rules" layout="vertical">
      <a-form-item :label="t('crm.codeLabel')" name="code">
        <a-input v-model:value="form.code" :placeholder="t('crm.codePlaceholder')" />
      </a-form-item>
      <a-form-item :label="t('crm.customerName')" name="name">
        <a-input v-model:value="form.name" :placeholder="t('crm.namePlaceholder')" />
      </a-form-item>
      <a-form-item :label="t('crm.level')" name="level">
        <a-select v-model:value="form.level" :options="levelOptions" />
      </a-form-item>
      <a-form-item :label="t('crm.phone')" name="phone">
        <a-input v-model:value="form.phone" :placeholder="t('crm.optional')" />
      </a-form-item>
      <a-form-item :label="t('crm.email')" name="email">
        <a-input v-model:value="form.email" :placeholder="t('crm.optional')" />
      </a-form-item>
      <a-form-item :label="t('crm.remark')" name="remark">
        <a-textarea v-model:value="form.remark" :rows="3" :placeholder="t('crm.optional')" />
      </a-form-item>
    </a-form>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 新建客户弹窗
//
// 客户编码租户内唯一；创建成功后 emit('saved') 让父页面刷新列表。
// =============================================================================

import { reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { FormInstance } from 'ant-design-vue';
import { ApiError, http } from '../../api/client';

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{
  (e: 'update:open', v: boolean): void;
  (e: 'saved'): void;
}>();

const { t } = useI18n();
const submitting = ref(false);
const formRef = ref<FormInstance>();
const levelOptions = [
  { label: t('crm.levelNormal'), value: 'normal' },
  { label: t('crm.levelImportant'), value: 'important' },
  { label: t('crm.levelStrategic'), value: 'strategic' },
];
const form = reactive({ code: '', name: '', level: 'normal', phone: '', email: '', remark: '' });
const rules = {
  code: [{ required: true, max: 32, message: t('crm.codeRule') }],
  name: [{ required: true, max: 128, message: t('crm.customerNameRule') }],
};

async function submit(): Promise<void> {
  try {
    await formRef.value?.validate();
  } catch {
    return;
  }
  submitting.value = true;
  try {
    await http.post('/api/crm/customers', {
      code: form.code.trim(),
      name: form.name.trim(),
      level: form.level,
      phone: form.phone.trim() || null,
      email: form.email.trim() || null,
      remark: form.remark.trim() || null,
    });
    message.success(t('crm.created'));
    emit('update:open', false);
    emit('saved');
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('crm.createFailed'));
  } finally {
    submitting.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    form.code = '';
    form.name = '';
    form.level = 'normal';
    form.phone = '';
    form.email = '';
    form.remark = '';
  },
);
</script>
