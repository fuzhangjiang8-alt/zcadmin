<template>
  <a-drawer
    :open="open"
    :title="t('roles.grantTitle', { name: role?.name ?? '' })"
    width="520"
    @update:open="emit('update:open', $event)"
  >
    <a-spin :spinning="loading">
      <a-alert
        type="info"
        show-icon
        :message="t('roles.grantHint')"
        style="margin-bottom: 16px"
      />
      <div v-for="group in grouped" :key="group.plugin" class="group">
        <div class="group-title">{{ group.plugin }}</div>
        <a-checkbox-group v-model:value="checkedIds" :options="group.options" />
      </div>
    </a-spin>

    <template #footer>
      <a-space>
        <a-button @click="emit('update:open', false)">{{ t('roles.cancel') }}</a-button>
        <a-button type="primary" :loading="saving" @click="save">{{ t('roles.save') }}</a-button>
      </a-space>
    </template>
  </a-drawer>
</template>

<script setup lang="ts">
// =============================================================================
// 角色授权抽屉
//
// 打开时拉取该角色已有的权限点 id；保存为全量覆盖。
// 权限点按插件分组展示，跳过已废弃的权限点。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';
import type { PermissionVo, RoleVo } from './types';

const props = defineProps<{
  open: boolean;
  role: RoleVo | null;
  permissions: PermissionVo[];
}>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

const { t } = useI18n();
const loading = ref(false);
const saving = ref(false);
const checkedIds = ref<string[]>([]);

const grouped = computed(() => {
  const map = new Map<string, { label: string; value: string }[]>();
  for (const p of props.permissions) {
    if (p.deprecated) continue;
    const list = map.get(p.plugin) ?? [];
    list.push({ label: `${p.name}（${p.code}）`, value: p.id });
    map.set(p.plugin, list);
  }
  return [...map.entries()].map(([plugin, options]) => ({ plugin, options }));
});

async function load(): Promise<void> {
  if (!props.role) return;
  loading.value = true;
  try {
    checkedIds.value = await http.get<string[]>(
      `/api/system/roles/${props.role.id}/permissions`,
    );
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('roles.grantLoadFailed'));
  } finally {
    loading.value = false;
  }
}

async function save(): Promise<void> {
  if (!props.role) return;
  saving.value = true;
  try {
    await http.put<string[]>(`/api/system/roles/${props.role.id}/permissions`, {
      ids: checkedIds.value,
    });
    message.success(t('roles.grantSaved'));
    emit('update:open', false);
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('roles.grantSaveFailed'));
  } finally {
    saving.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      void load();
    }
  },
);
</script>

<style scoped>
.group {
  margin-bottom: 20px;
}

.group-title {
  font-weight: 600;
  margin-bottom: 8px;
  color: #334155;
}
</style>
