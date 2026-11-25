<template>
  <a-modal
    :open="open"
    :title="t('roles.scopeTitle')"
    :footer="null"
    width="640px"
    @update:open="emit('update:open', $event)"
  >
    <a-select
      v-model:value="sampleDeptId"
      :options="deptOptions"
      :placeholder="t('roles.scopeDeptPlaceholder')"
      allow-clear
      style="width: 100%; margin-bottom: 16px"
      @change="loadPreview"
    />
    <a-spin :spinning="loading">
      <a-descriptions :column="2" size="small" bordered>
        <a-descriptions-item :label="t('roles.descRole')">{{ role?.name }}</a-descriptions-item>
        <a-descriptions-item :label="t('roles.descScope')">{{ scopeText(role?.dataScope ?? '') }}</a-descriptions-item>
        <a-descriptions-item :label="t('roles.descDescription')" :span="2">
          {{ preview?.description ?? '—' }}
        </a-descriptions-item>
        <a-descriptions-item :label="t('roles.descVisibleDepts')">
          {{ preview?.visibleDeptIds.length ?? 0 }}
        </a-descriptions-item>
        <a-descriptions-item :label="t('roles.descUsers')">{{ preview?.userCount ?? 0 }}</a-descriptions-item>
      </a-descriptions>
      <div v-if="visibleDeptNames.length > 0" class="scope-depts">
        <a-tag v-for="name in visibleDeptNames" :key="name" color="blue">{{ name }}</a-tag>
      </div>
    </a-spin>
  </a-modal>
</template>

<script setup lang="ts">
// =============================================================================
// 角色数据范围预览
//
// 独立组件：打开时按需加载部门树，dataScope 为 all/self 时自动预览；
// 其他范围需要用户先选一个示例部门（后端按该部门计算可见范围）。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../../api/client';
import { scopeText } from './types';
import type { DeptNode, RoleVo, ScopePreview } from './types';

const props = defineProps<{ open: boolean; role: RoleVo | null }>();
const emit = defineEmits<{ (e: 'update:open', v: boolean): void }>();

const { t } = useI18n();
const preview = ref<ScopePreview | null>(null);
const loading = ref(false);
const sampleDeptId = ref<string | undefined>(undefined);
const deptFlat = ref<{ id: string; name: string }[]>([]);

const deptOptions = computed(() => deptFlat.value.map((d) => ({ label: d.name, value: d.id })));

const visibleDeptNames = computed(() => {
  if (!preview.value) return [];
  const byId = new Map(deptFlat.value.map((d) => [d.id, d.name]));
  return preview.value.visibleDeptIds.map((id) => byId.get(id) ?? id);
});

function flatten(nodes: DeptNode[], out: { id: string; name: string }[] = []) {
  for (const n of nodes) {
    out.push({ id: n.id, name: n.name });
    flatten(n.children ?? [], out);
  }
  return out;
}

async function loadPreview(): Promise<void> {
  if (!props.role) return;
  loading.value = true;
  try {
    const query = sampleDeptId.value ? `?deptId=${sampleDeptId.value}` : '';
    preview.value = await http.get<ScopePreview>(
      `/api/system/roles/${props.role.id}/scope-preview${query}`,
    );
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('roles.previewFailed'));
  } finally {
    loading.value = false;
  }
}

async function init(): Promise<void> {
  preview.value = null;
  sampleDeptId.value = undefined;
  if (deptFlat.value.length === 0) {
    const tree = await http.get<DeptNode[]>('/api/system/depts');
    deptFlat.value = flatten(tree);
  }
  if (props.role && (props.role.dataScope === 'all' || props.role.dataScope === 'self')) {
    await loadPreview();
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) {
      void init();
    }
  },
);
</script>

<style scoped>
.scope-depts {
  margin-top: 12px;
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
</style>
