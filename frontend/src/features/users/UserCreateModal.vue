<template>
  <a-modal
    :open="open"
    :title="user ? t('users.editTitle') : t('users.createTitle')"
    :confirm-loading="submitting"
    :ok-text="user ? t('users.save') : t('users.okCreate')"
    :cancel-text="t('users.cancel')"
    @ok="submit"
    @update:open="emit('update:open', $event)"
  >
    <a-form ref="formRef" :model="form" :rules="rules" layout="vertical">
      <a-form-item :label="t('users.username')" name="username">
        <a-input v-model:value="form.username" :disabled="!!user" :placeholder="t('users.usernamePlaceholder')" />
      </a-form-item>
      <a-form-item :label="t('users.displayName')" name="displayName">
        <a-input v-model:value="form.displayName" :placeholder="t('users.displayNamePlaceholder')" />
      </a-form-item>
      <a-form-item :label="user ? t('users.resetPasswordLabel') : t('users.password')" name="password">
        <a-input-password
          v-model:value="form.password"
          :placeholder="user ? t('users.resetPasswordPlaceholder') : t('users.passwordPlaceholder')"
        />
      </a-form-item>
      <a-form-item :label="t('users.email')" name="email">
        <a-input v-model:value="form.email" :placeholder="t('users.optional')" />
      </a-form-item>
      <a-form-item :label="t('users.phone')" name="phone">
        <a-input v-model:value="form.phone" :placeholder="t('users.optional')" />
      </a-form-item>
      <a-form-item v-if="!user" :label="t('users.firstLogin')">
        <a-checkbox v-model:checked="form.mustChangePassword">{{ t('users.mustChangePassword') }}</a-checkbox>
      </a-form-item>
      <a-form-item :label="t('users.dept')" name="deptId">
        <a-select
          v-model:value="form.deptId"
          :options="deptOptions"
          :placeholder="t('users.deptPlaceholder')"
          allow-clear
        />
      </a-form-item>
    </a-form>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 新建用户弹窗
//
// 打开时加载部门树；提交成功后 emit('saved') 让父页面刷新列表。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { FormInstance } from 'ant-design-vue';
import { ApiError, http } from '../../api/client';
import { flattenDepts } from './types';
import type { DeptNode, UserVo } from './types';

const props = defineProps<{ open: boolean; user?: UserVo | null }>();
const emit = defineEmits<{
  (e: 'update:open', v: boolean): void;
  (e: 'saved'): void;
}>();

const { t } = useI18n();
const submitting = ref(false);
const formRef = ref<FormInstance>();
const depts = ref<DeptNode[]>([]);
const deptOptions = computed(() => flattenDepts(depts.value));

const form = reactive({
  username: '',
  displayName: '',
  password: '',
  email: '',
  phone: '',
  deptId: undefined as string | undefined,
  mustChangePassword: true,
});

const rules = computed(() => ({
  username: [{ required: true, min: 3, max: 64, message: t('users.usernameRule') }],
  displayName: [{ required: true, message: t('users.displayNameRequired') }],
  password: props.user ? [] : [{ required: true, min: 10, message: t('users.passwordRule') }],
  phone: [{ pattern: /^(\+?86)?1[3-9]\d{9}$/, message: t('users.phoneRule') }],
}));

function reset(): void {
  form.username = props.user?.username ?? '';
  form.displayName = props.user?.displayName ?? '';
  form.password = '';
  form.email = props.user?.email && !props.user.email.includes('*') ? props.user.email : '';
  form.phone = props.user?.phone && !props.user.phone.includes('*') ? props.user.phone : '';
  form.deptId = props.user?.deptId ?? undefined;
  form.mustChangePassword = true;
}

async function loadDepts(): Promise<void> {
  try {
    depts.value = await http.get<DeptNode[]>('/api/system/depts');
  } catch {
    // 部门加载失败不阻塞创建；下拉为空即可
  }
}

async function submit(): Promise<void> {
  try {
    await formRef.value?.validate();
  } catch {
    return;
  }
  submitting.value = true;
  try {
    if (props.user) {
      await http.put<UserVo>(`/api/system/users/${props.user.id}`, {
        displayName: form.displayName.trim(),
        email: form.email.trim() || null,
        phone: form.phone.trim() || null,
        deptId: form.deptId ?? null,
      });
      if (form.password.trim()) {
        await http.post(`/api/system/users/${props.user.id}/reset-password`, {
          password: form.password,
        });
      }
    } else {
      await http.post<UserVo>('/api/system/users', {
        username: form.username.trim(),
        displayName: form.displayName.trim(),
        password: form.password,
        email: form.email.trim() || null,
        phone: form.phone.trim() || null,
        deptId: form.deptId ?? null,
        mustChangePassword: form.mustChangePassword,
      });
    }
    message.success(t('users.created'));
    emit('update:open', false);
    emit('saved');
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('users.createFailed'));
  } finally {
    submitting.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      reset();
      void loadDepts();
    }
  },
);
</script>
