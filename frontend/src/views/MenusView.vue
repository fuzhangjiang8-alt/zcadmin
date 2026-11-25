<template>
  <div class="page">
    <a-card :bordered="false" :title="t('menus.title')">
      <template #extra>
        <a-button v-perm="PERMS.MENU_ADD" type="primary" @click="openCreate(null)">
          {{ t('menus.new') }}
        </a-button>
      </template>

      <a-alert
        type="info"
        show-icon
        :message="t('menus.info')"
        style="margin-bottom: 16px"
      />

      <a-table
        :columns="columns"
        :data-source="tree"
        :loading="loading"
        row-key="id"
        :pagination="false"
        default-expand-all-rows
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'visible'">
            <a-tag :color="record.visible ? 'green' : 'default'">
              {{ record.visible ? t('menus.visible') : t('menus.hidden') }}
            </a-tag>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-space>
              <a-button v-perm="PERMS.MENU_ADD" type="link" size="small" @click="openCreate(record)">
                {{ t('menus.addChild') }}
              </a-button>
              <a-button v-perm="PERMS.MENU_EDIT" type="link" size="small" @click="openEdit(record)">
                {{ t('menus.edit') }}
              </a-button>
              <a-popconfirm :title="t('menus.deleteConfirm')" @confirm="onDelete(record)">
                <a-button v-perm="PERMS.MENU_DELETE" type="link" size="small" danger>
                  {{ t('menus.delete') }}
                </a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>

    <a-modal
      v-model:open="editOpen"
      :title="editing ? t('menus.editTitle') : t('menus.newTitle')"
      :confirm-loading="submitting"
      width="560px"
      @ok="submit"
    >
      <a-form ref="formRef" :model="form" :rules="rules" layout="vertical">
        <a-form-item :label="t('menus.parent')" name="parentId">
          <a-select
            v-model:value="form.parentId"
            allow-clear
            :placeholder="t('menus.rootOption')"
            :options="parentOptions"
          />
        </a-form-item>
        <a-form-item :label="t('menus.titleLabel')" name="title">
          <a-input v-model:value="form.title" :placeholder="t('menus.titlePlaceholder')" />
        </a-form-item>
        <a-form-item :label="t('menus.path')" name="path">
          <a-input v-model:value="form.path" placeholder="/crm/customers" />
        </a-form-item>
        <a-form-item :label="t('menus.perm')" name="perm">
          <a-input v-model:value="form.perm" :placeholder="t('menus.permPlaceholder')" />
        </a-form-item>
        <a-form-item :label="t('menus.icon')" name="icon">
          <a-input v-model:value="form.icon" :placeholder="t('menus.iconPlaceholder')" />
        </a-form-item>
        <a-form-item :label="t('menus.sort')" name="sort">
          <a-input-number v-model:value="form.sort" :min="0" :max="9999" style="width: 160px" />
        </a-form-item>
        <a-form-item :label="t('menus.visibleLabel')" name="visible">
          <a-switch v-model:checked="form.visible" />
        </a-form-item>
      </a-form>
    </a-modal>
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../permissions';
// =============================================================================
// 菜单管理
//
// 后端返回平铺列表，前端组树。父菜单被隐藏时子菜单也不会出现在侧边栏，
// 但管理页仍能看到（否则无法恢复）。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { FormInstance } from 'ant-design-vue';
import { ApiError, http } from '../api/client';

interface MenuRow {
  id: string;
  parentId: string | null;
  title: string;
  path: string;
  icon: string | null;
  perm: string | null;
  sort: number;
  visible: boolean;
}

interface MenuNode extends MenuRow {
  children: MenuNode[];
}

const { t } = useI18n();
const queryClient = useQueryClient();

const query = useQuery({
  queryKey: ['menus-all'],
  queryFn: () => http.get<MenuRow[]>('/api/system/menus/all'),
});
const rows = computed(() => query.data.value ?? []);
const loading = computed(() => query.isFetching.value);
watch(query.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('menus.loadFailed'));
});

const columns = computed(() => [
  { title: t('menus.colTitle'), dataIndex: 'title', key: 'title' },
  { title: t('menus.colPath'), dataIndex: 'path', key: 'path' },
  { title: t('menus.colPerm'), dataIndex: 'perm', key: 'perm', width: 220 },
  { title: t('menus.colSort'), dataIndex: 'sort', key: 'sort', width: 80 },
  { title: t('menus.colStatus'), dataIndex: 'visible', key: 'visible', width: 90 },
  { title: t('menus.colActions'), key: 'actions', width: 240 },
]);

/** 平铺 → 树。 */
const tree = computed<MenuNode[]>(() => {
  const map = new Map<string, MenuNode>();
  rows.value.forEach((r) => map.set(r.id, { ...r, children: [] }));
  const roots: MenuNode[] = [];
  map.forEach((node) => {
    if (node.parentId && map.has(node.parentId)) {
      map.get(node.parentId)!.children.push(node);
    } else {
      roots.push(node);
    }
  });
  return roots;
});

/** 上级菜单选项：编辑时排除自己（后端还会阻止选到后代）。 */
const parentOptions = computed(() =>
  rows.value
    .filter((r) => !editing.value || r.id !== editing.value.id)
    .map((r) => ({ label: r.title, value: r.id })),
);

// ---- 新建 / 编辑 ----
const editOpen = ref(false);
const submitting = ref(false);
const editing = ref<MenuRow | null>(null);
const formRef = ref<FormInstance>();
const form = reactive({
  parentId: null as string | null,
  title: '',
  path: '',
  perm: '',
  icon: '',
  sort: 0,
  visible: true,
});

const rules = {
  title: [{ required: true, max: 64, message: t('menus.titleRule') }],
  path: [
    { required: true, message: t('menus.pathRequired') },
    { pattern: /^\//, message: t('menus.pathSlash') },
  ],
};

function openCreate(parent: MenuNode | null): void {
  editing.value = null;
  form.parentId = parent?.id ?? null;
  form.title = '';
  form.path = '';
  form.perm = '';
  form.icon = '';
  form.sort = 0;
  form.visible = true;
  editOpen.value = true;
}

function openEdit(row: MenuRow): void {
  editing.value = row;
  form.parentId = row.parentId;
  form.title = row.title;
  form.path = row.path;
  form.perm = row.perm ?? '';
  form.icon = row.icon ?? '';
  form.sort = row.sort;
  form.visible = row.visible;
  editOpen.value = true;
}

async function submit(): Promise<void> {
  try {
    await formRef.value?.validate();
  } catch {
    return;
  }
  submitting.value = true;
  const payload = {
    parentId: form.parentId,
    title: form.title.trim(),
    path: form.path.trim(),
    perm: form.perm.trim() || null,
    icon: form.icon.trim() || null,
    sort: form.sort,
    visible: form.visible,
  };
  try {
    if (editing.value) {
      await http.put<MenuRow>(`/api/system/menus/${editing.value.id}`, payload);
    } else {
      await http.post<MenuRow>('/api/system/menus', payload);
    }
    message.success(t('menus.saved'));
    editOpen.value = false;
    void queryClient.invalidateQueries({ queryKey: ['menus-all'] });
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('menus.saveFailed'));
  } finally {
    submitting.value = false;
  }
}

const deleteMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/system/menus/${id}`),
  onSuccess: () => {
    message.success(t('menus.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['menus-all'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('menus.deleteFailed'));
  },
});

function onDelete(row: MenuRow): void {
  deleteMutation.mutate(row.id);
}

</script>

<style scoped>
.page {
  padding: 0;
}
</style>
