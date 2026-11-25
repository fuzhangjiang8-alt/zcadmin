<template>
  <div class="page">
    <a-card :bordered="false" :title="t('dept.title')">
      <template #extra>
        <a-button v-perm="PERMS.DEPT_ADD" type="primary" @click="openCreate()">
          {{ t('dept.new') }}
        </a-button>
      </template>

      <a-table
        :columns="columns"
        :data-source="depts"
        :loading="loading"
        row-key="id"
        :pagination="false"
        default-expand-all-rows
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'actions'">
            <a-space>
              <a-button v-perm="PERMS.DEPT_ADD" type="link" size="small" @click="openCreate(record)">
                {{ t('dept.addChild') }}
              </a-button>
              <a-button v-perm="PERMS.DEPT_EDIT" type="link" size="small" @click="openEdit(record)">
                {{ t('dept.edit') }}
              </a-button>
              <a-popconfirm :title="t('dept.deleteConfirm')" @confirm="onDelete(record)">
                <a-button v-perm="PERMS.DEPT_DELETE" type="link" size="small" danger>
                  {{ t('dept.delete') }}
                </a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>

    <a-modal
      v-model:open="createOpen"
      :title="editing ? t('dept.editTitle') : parent ? t('dept.newChildTitle', { name: parent.name }) : t('dept.newTitle')"
      :confirm-loading="submitting"
      @ok="submit"
    >
      <a-form ref="formRef" :model="form" :rules="rules" layout="vertical">
        <a-form-item :label="t('dept.name')" name="name">
          <a-input v-model:value="form.name" :placeholder="t('dept.namePlaceholder')" />
        </a-form-item>
        <a-form-item :label="t('dept.sort')" name="sort">
          <a-input-number v-model:value="form.sort" :min="0" :max="9999" style="width: 160px" />
        </a-form-item>
      </a-form>
    </a-modal>
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../permissions';
// =============================================================================
// 部门管理（树形）
//
// 数据权限的划分依据：用户挂到部门后，角色的 data_scope 才能生效。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { FormInstance } from 'ant-design-vue';
import { ApiError, http } from '../api/client';

interface DeptNode {
  id: string;
  name: string;
  sort?: number;
  children: DeptNode[];
}

const { t } = useI18n();
const queryClient = useQueryClient();

const query = useQuery({
  queryKey: ['depts'],
  queryFn: () => http.get<DeptNode[]>('/api/system/depts'),
});
const depts = computed(() => query.data.value ?? []);
const loading = computed(() => query.isFetching.value);

watch(query.error, (error) => {
  if (error) {
    message.error(error instanceof ApiError ? error.message : t('dept.loadFailed'));
  }
});

const columns = computed(() => [
  { title: t('dept.colName'), dataIndex: 'name', key: 'name' },
  { title: t('dept.sort'), dataIndex: 'sort', key: 'sort', width: 100 },
  { title: t('dept.colActions'), key: 'actions', width: 180 },
]);

const createOpen = ref(false);
const submitting = ref(false);
const formRef = ref<FormInstance>();
const parent = ref<DeptNode | null>(null);
const editing = ref<DeptNode | null>(null);
const form = reactive({ name: '', sort: 0 });
const rules = { name: [{ required: true, max: 64, message: t('dept.nameRule') }] };

function openCreate(node?: DeptNode): void {
  editing.value = null;
  parent.value = node ?? null;
  form.name = '';
  form.sort = 0;
  createOpen.value = true;
}

function openEdit(node: DeptNode): void {
  editing.value = node;
  parent.value = null;
  form.name = node.name;
  form.sort = node.sort ?? 0;
  createOpen.value = true;
}

const saveMutation = useMutation({
  mutationFn: async () => {
    if (editing.value) {
      return http.put<DeptNode>(`/api/system/depts/${editing.value.id}`, {
        name: form.name.trim(),
        sort: form.sort,
      });
    }
    return http.post<DeptNode>('/api/system/depts', {
      name: form.name.trim(),
      parentId: parent.value?.id ?? null,
      sort: form.sort,
    });
  },
  onSuccess: () => {
    message.success(t('dept.created'));
    createOpen.value = false;
    void queryClient.invalidateQueries({ queryKey: ['depts'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('dept.createFailed'));
  },
  onSettled: () => {
    submitting.value = false;
  },
});

async function submit(): Promise<void> {
  try {
    await formRef.value?.validate();
  } catch {
    return;
  }
  submitting.value = true;
  saveMutation.mutate();
}

const deleteMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/system/depts/${id}`),
  onSuccess: () => {
    message.success(t('dept.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['depts'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('dept.deleteFailed'));
  },
});

function onDelete(node: DeptNode): void {
  deleteMutation.mutate(node.id);
}
</script>

<style scoped>
.page {
  padding: 0;
}
</style>
