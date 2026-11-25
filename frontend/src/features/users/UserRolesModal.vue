<template>
  <a-modal
    :open="open"
    :title="t('users.rolesTitle', { name: user?.username ?? '' })"
    :confirm-loading="saving"
    :ok-text="t('users.save')"
    :cancel-text="t('users.cancel')"
    @ok="save"
    @update:open="emit('update:open', $event)"
  >
    <a-spin :spinning="loading">
      <a-checkbox-group v-model:value="checkedIds" :options="options" class="role-group" />
      <a-empty v-if="options.length === 0" :description="t('users.noRoles')" />
    </a-spin>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 用户角色分配
//
// 打开时并行加载「全部角色」和「该用户已有角色」；保存为全量覆盖。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';
import type { RoleVo, UserVo } from './types';

const props = defineProps<{ open: boolean; user: UserVo | null }>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

const { t } = useI18n();
const loading = ref(false);
const saving = ref(false);
const roles = ref<RoleVo[]>([]);
const checkedIds = ref<string[]>([]);

const options = computed(() => roles.value.map((r) => ({ label: `${r.name}（${r.code}）`, value: r.id })));

async function load(): Promise<void> {
  if (!props.user) return;
  loading.value = true;
  try {
    const [all, mine] = await Promise.all([
      http.get<RoleVo[]>('/api/system/roles'),
      http.get<string[]>(`/api/system/users/${props.user.id}/roles`),
    ]);
    roles.value = all;
    checkedIds.value = mine;
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('users.rolesLoadFailed'));
  } finally {
    loading.value = false;
  }
}

async function save(): Promise<void> {
  if (!props.user) return;
  saving.value = true;
  try {
    await http.put<string[]>(`/api/system/users/${props.user.id}/roles`, {
      ids: checkedIds.value,
    });
    message.success(t('users.rolesSaved'));
    emit('update:open', false);
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('users.saveFailedShort'));
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

<style scoped>
.role-group {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
</style>
