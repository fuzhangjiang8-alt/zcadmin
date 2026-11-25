<template>
  <div class="page">
    <a-card :bordered="false" :title="t('roles.title')">
      <template #extra>
        <a-button v-perm="PERMS.ROLE_ADD" type="primary" @click="openCreate">{{ t('roles.new') }}</a-button>
      </template>

      <a-table :columns="columns" :data-source="roles" :loading="loading" row-key="id">
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'dataScope'">
            <a-tag :color="scopeColor(record.dataScope)">{{ scopeText(record.dataScope) }}</a-tag>
          </template>
          <template v-else-if="column.key === 'isSystem'">
            <a-tag v-if="record.isSystem" color="gold">{{ t('roles.system') }}</a-tag>
            <span v-else class="muted">—</span>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-space>
              <a-button v-perm="PERMS.ROLE_EDIT" type="link" size="small" @click="openEdit(record)">
                {{ t('roles.edit') }}
              </a-button>
              <a-button type="link" size="small" :disabled="record.isSystem" @click="openGrant(record)">
                {{ t('roles.grant') }}
              </a-button>
              <a-button type="link" size="small" @click="openScope(record)">{{ t('roles.scope') }}</a-button>
              <a-button type="link" size="small" @click="openDepts(record)">{{ t('roles.depts') }}</a-button>
              <a-popconfirm :title="t('roles.deleteConfirm')" @confirm="onDelete(record)">
                <a-button v-perm="PERMS.ROLE_EDIT" type="link" danger size="small" :disabled="record.isSystem">
                  {{ t('roles.delete') }}
                </a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>

    <a-modal v-model:open="createOpen" :title="editingRole ? t('roles.editTitle') : t('roles.createTitle')" :confirm-loading="submitting" @ok="submitCreate">
      <a-form ref="formRef" :model="form" :rules="rules" layout="vertical">
        <a-form-item :label="t('roles.code')" name="code"><a-input v-model:value="form.code" :disabled="!!editingRole" :placeholder="t('roles.codePlaceholder')" /></a-form-item>
        <a-form-item :label="t('roles.name')" name="name"><a-input v-model:value="form.name" :placeholder="t('roles.namePlaceholder')" /></a-form-item>
        <a-form-item :label="t('roles.scopeLabel')" name="dataScope"><a-select v-model:value="form.dataScope" :disabled="!!editingRole?.isSystem" :options="scopeOptions" /></a-form-item>
        <a-form-item :label="t('roles.description')" name="description"><a-textarea v-model:value="form.description" :rows="3" /></a-form-item>
      </a-form>
    </a-modal>

    <RoleScopeModal v-model:open="scopeOpen" :role="scopeRole" />
    <RoleGrantDrawer v-model:open="grantOpen" :role="grantRole" :permissions="permissions" />

    <a-modal v-model:open="deptOpen" :title="t('roles.deptsTitle')" :confirm-loading="deptSaving" @ok="saveDepts">
      <a-checkbox-group v-model:value="checkedDeptIds" :options="deptOptions" class="dept-group" />
      <div class="muted" style="margin-top: 8px">{{ t('roles.deptsHint') }}</div>
    </a-modal>
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../permissions';
// =============================================================================
// 角色管理
//
// 三个动作：
//   1. 列出角色
//   2. 新建角色
//   3. 给角色勾选权限点（全量覆盖）
//
// 注意：内置角色（is_system）不允许授权，避免把超级管理员改坏。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { FormInstance } from 'ant-design-vue';
import { ApiError, http } from '../api/client';
import RoleGrantDrawer from '../features/roles/RoleGrantDrawer.vue';
import RoleScopeModal from '../features/roles/RoleScopeModal.vue';
import { scopeText } from '../features/roles/types';
import type { DeptNode, PermissionVo, RoleVo } from '../features/roles/types';

const { t } = useI18n();
const queryClient = useQueryClient();

const rolesQuery = useQuery({
  queryKey: ['roles'],
  queryFn: () => http.get<RoleVo[]>('/api/system/roles'),
});
const permissionsQuery = useQuery({
  queryKey: ['permissions'],
  queryFn: () => http.get<PermissionVo[]>('/api/system/permissions'),
});
const roles = computed(() => rolesQuery.data.value ?? []);
const permissions = computed(() => permissionsQuery.data.value ?? []);
const loading = computed(
  () => rolesQuery.isFetching.value || permissionsQuery.isFetching.value,
);

watch([rolesQuery.error, permissionsQuery.error], ([roleError, permError]) => {
  const error = roleError ?? permError;
  if (error) {
    message.error(error instanceof ApiError ? error.message : t('roles.loadFailed'));
  }
});

const columns = computed(() => [
  { title: t('roles.colCode'), dataIndex: 'code', key: 'code', width: 180 },
  { title: t('roles.colName'), dataIndex: 'name', key: 'name' },
  { title: t('roles.colDescription'), dataIndex: 'description', key: 'description' },
  { title: t('roles.colScope'), dataIndex: 'dataScope', key: 'dataScope', width: 140 },
  { title: t('roles.colType'), dataIndex: 'isSystem', key: 'isSystem', width: 90 },
  { title: t('roles.colActions'), key: 'actions', width: 100 },
]);

function scopeColor(scope: string): string {
  return (
    { all: 'red', dept_and_child: 'orange', custom: 'purple', dept: 'blue', self: 'default' }[
      scope
    ] ?? 'default'
  );
}

// ---- 新建/编辑角色 ----
const createOpen = ref(false);
const editingRole = ref<RoleVo | null>(null);
const submitting = ref(false);
const formRef = ref<FormInstance>();
const form = reactive({ code: '', name: '', description: '', dataScope: 'self' });
const rules = {
  code: [{ required: true, min: 2, max: 64, message: t('roles.codeRule') }],
  name: [{ required: true, message: t('roles.nameRequired') }],
};

/** 数据范围选项：权限从宽到窄，默认 self。 */
const scopeOptions = computed(() => [
  { label: t('roles.scopeAll'), value: 'all' },
  { label: t('roles.scopeDeptAndChild'), value: 'dept_and_child' },
  { label: t('roles.scopeDept'), value: 'dept' },
  { label: t('roles.scopeCustom'), value: 'custom' },
  { label: t('roles.scopeSelf'), value: 'self' },
]);

const deptOpen = ref(false);
const deptRole = ref<RoleVo | null>(null);
const checkedDeptIds = ref<string[]>([]);
const deptSaving = ref(false);

async function openDepts(record: RoleVo): Promise<void> {
  deptRole.value = record;
  try {
    await loadDepts();
    checkedDeptIds.value = await http.get<string[]>( `/api/system/roles/${record.id}/depts`);
    deptOpen.value = true;
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('roles.loadDeptsFailed'));
  }
}

async function saveDepts(): Promise<void> {
  if (!deptRole.value) return;
  deptSaving.value = true;
  try {
    await http.put<string[]>(`/api/system/roles/${deptRole.value.id}/depts`, {
      deptIds: checkedDeptIds.value,
    });
    message.success(t('roles.saved'));
    deptOpen.value = false;
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('roles.saveFailed'));
  } finally {
    deptSaving.value = false;
  }
}

function openCreate(): void {
  editingRole.value = null;
  Object.assign(form, { code: '', name: '', description: '', dataScope: 'self' });
  createOpen.value = true;
}

function openEdit(record: RoleVo): void {
  editingRole.value = record;
  Object.assign(form, {
    code: record.code,
    name: record.name,
    description: record.description ?? '',
    dataScope: record.dataScope,
  });
  createOpen.value = true;
}

const createMutation = useMutation({
  mutationFn: () => {
    if (editingRole.value) {
      return http.put<RoleVo>(`/api/system/roles/${editingRole.value.id}`, {
        name: form.name.trim(),
        description: form.description.trim() || null,
        dataScope: form.dataScope,
      });
    }
    return http.post<RoleVo>('/api/system/roles', {
      code: form.code.trim(),
      name: form.name.trim(),
      description: form.description.trim() || null,
      dataScope: form.dataScope,
    });
  },
  onSuccess: () => {
    message.success(editingRole.value ? t('roles.saved') : t('roles.created'));
    createOpen.value = false;
    void queryClient.invalidateQueries({ queryKey: ['roles'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('roles.saveFailed'));
  },
  onSettled: () => {
    submitting.value = false;
  },
});

const deleteRoleMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/system/roles/${id}`),
  onSuccess: () => {
    message.success(t('roles.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['roles'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('roles.deleteFailed'));
  },
});

function onDelete(record: RoleVo): void {
  deleteRoleMutation.mutate(record.id);
}

async function submitCreate(): Promise<void> {
  try {
    await formRef.value?.validate();
  } catch {
    return;
  }
  submitting.value = true;
  createMutation.mutate();
}

// ---- 授权 ----
const grantOpen = ref(false);
const grantRole = ref<RoleVo | null>(null);

function openGrant(role: RoleVo): void {
  grantRole.value = role;
  grantOpen.value = true;
}

// ---- 数据范围预览 / 指定部门（部门选项共用）----
const scopeOpen = ref(false);
const scopeRole = ref<RoleVo | null>(null);

const deptFlat = ref<{ id: string; name: string }[]>([]);
const deptOptions = computed(() => deptFlat.value.map((d) => ({ label: d.name, value: d.id })));

function flatten(nodes: DeptNode[], out: { id: string; name: string }[] = []) {
  for (const n of nodes) {
    out.push({ id: n.id, name: n.name });
    flatten(n.children ?? [], out);
  }
  return out;
}

async function loadDepts(): Promise<void> {
  if (deptFlat.value.length > 0) return;
  const tree = await http.get<DeptNode[]>('/api/system/depts');
  deptFlat.value = flatten(tree);
}

function openScope(role: RoleVo): void {
  scopeRole.value = role;
  scopeOpen.value = true;
}

</script>

<style scoped>
.page { padding: 0; }
.group { margin-bottom: 20px; }
.group-title { font-weight: 600; margin-bottom: 8px; color: #334155; }
.muted { color: #cbd5e1; }
</style>
