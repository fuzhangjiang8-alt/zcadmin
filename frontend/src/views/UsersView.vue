<template>
  <div class="page">
    <a-card :bordered="false">
      <template #title>
        <div class="card-title">
          <span>{{ t('users.pageTitle') }}</span>
          <a-tag color="blue">{{ t('users.peopleCount', { n: total }) }}</a-tag>
        </div>
      </template>

      <template #extra>
        <a-space>
          <a-input-search v-model:value="searchText" :placeholder="t('users.searchPlaceholder')" allow-clear style="width: 240px" @search="onSearch" />
          <a-button v-perm="PERMS.EXPORT_LIST" :loading="exporting" @click="onExport">{{ t('users.export') }}</a-button>
          <a-button v-perm="PERMS.USER_ADD" @click="downloadTemplate">{{ t('users.template') }}</a-button>
          <a-checkbox v-model:checked="dryRun">{{ t('users.dryRun') }}</a-checkbox>
          <a-upload v-perm="PERMS.USER_ADD" :custom-request="onImport" :show-upload-list="false" accept=".xlsx">
            <a-button v-perm="PERMS.USER_ADD" :loading="importing">{{ t('users.importBtn') }}</a-button>
          </a-upload>
          <a-button v-perm="PERMS.CONFIG_EDIT" @click="policyOpen = true">{{ t('users.passwordPolicyBtn') }}</a-button>
          <a-button v-perm="PERMS.CONFIG_EDIT" @click="loginPolicyOpen = true">{{ t('users.loginPolicyBtn') }}</a-button>
          <a-button v-perm="PERMS.USER_ADD" type="primary" @click="openCreate">{{ t('users.newUser') }}</a-button>
        </a-space>
      </template>

      <a-table
        :columns="columns"
        :data-source="users"
        :loading="loading"
        row-key="id"
        :pagination="{ current: page, pageSize: size, total, showSizeChanger: true, showTotal: (total: number) => t('users.totalCount', { n: total }) }"
        @change="onTableChange"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'status'">
            <a-tag :color="statusColor(record.status)">{{ statusText(record.status) }}</a-tag>
          </template>
          <template v-else-if="column.key === 'isSuper'">
            <a-tag v-if="record.isSuper" color="gold">{{ t('users.superAdmin') }}</a-tag>
            <span v-else class="muted">—</span>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-space>
              <a-button v-perm="PERMS.USER_EDIT" type="link" size="small" @click="openEdit(record)">{{ t('users.edit') }}</a-button>
              <a-button type="link" size="small" :disabled="record.isSuper" @click="openRoles(record)">{{ t('users.assignRoles') }}</a-button>
              <a-button type="link" size="small" @click="openPosts(record)">{{ t('users.posts') }}</a-button>
              <a-button v-perm="PERMS.USER_EDIT" type="link" size="small" :disabled="record.isSuper" @click="toggleStatus(record)">
                {{ record.status === 'active' ? t('users.disable') : t('users.enable') }}
              </a-button>
              <a-popconfirm :title="t('users.unlockConfirm')" @confirm="unlock(record)">
                <a-button v-perm="PERMS.USER_EDIT" type="link" size="small">{{ t('users.unlock') }}</a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>

    <UserCreateModal v-model:open="createOpen" :user="editingUser" @saved="reload" />
    <UserRolesModal v-model:open="roleOpen" :user="activeUser" />
    <UserPostsModal v-model:open="postOpen" :user="activeUser" @saved="reload" />
    <PasswordPolicyModal v-model:open="policyOpen" />
    <LoginPolicyModal v-model:open="loginPolicyOpen" />
    <ImportResultModal v-model:open="importOpen" :result="importResult" />
  </div>
</template>

<script setup lang="ts">
// =============================================================================
// 用户管理页
//
// 页面只保留：工具栏、列表、分页、导入/导出编排；
// 各弹窗（新建/角色/岗位/密码策略/登录策略/导入结果）拆到 features/users。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { TablePaginationConfig } from 'ant-design-vue';
import { ApiError, http } from '../api/client';
import { PERMS } from '../permissions';
import ImportResultModal from '../features/users/ImportResultModal.vue';
import LoginPolicyModal from '../features/users/LoginPolicyModal.vue';
import PasswordPolicyModal from '../features/users/PasswordPolicyModal.vue';
import UserCreateModal from '../features/users/UserCreateModal.vue';
import UserPostsModal from '../features/users/UserPostsModal.vue';
import UserRolesModal from '../features/users/UserRolesModal.vue';
import type { ImportResult, UserVo } from '../features/users/types';

interface PageResult<T> {
  data: T[];
  page: { page: number; size: number; total: number; hasNext: boolean };
}

interface ExportTaskVo {
  id: string;
  status: string;
  error: string | null;
}

interface UploadRequestOption {
  file: File;
  onSuccess?: (body: unknown) => void;
  onError?: (err: Error) => void;
}

const { t } = useI18n();
const queryClient = useQueryClient();
const searchText = ref('');
const keyword = ref('');
const page = ref(1);
const size = ref(20);

const query = useQuery({
  queryKey: computed(() => ['users', page.value, size.value, keyword.value] as const),
  queryFn: async () => {
    const params = new URLSearchParams({ page: String(page.value), size: String(size.value) });
    if (keyword.value.trim()) params.set('keyword', keyword.value.trim());
    return http.get<PageResult<UserVo>>(`/api/system/users?${params.toString()}`);
  },
  placeholderData: keepPreviousData,
});

const users = computed(() => query.data.value?.data ?? []);
const total = computed(() => query.data.value?.page.total ?? 0);
const loading = computed(() => query.isFetching.value);

watch(query.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('users.loadFailed'));
});

const createOpen = ref(false);
const editingUser = ref<UserVo | null>(null);
const roleOpen = ref(false);
const postOpen = ref(false);
const policyOpen = ref(false);
const loginPolicyOpen = ref(false);
const activeUser = ref<UserVo | null>(null);

const exporting = ref(false);
const importing = ref(false);
const dryRun = ref(false);
const importOpen = ref(false);
const importResult = ref<ImportResult | null>(null);

const columns = computed(() => [
  { title: t('users.username'), dataIndex: 'username', key: 'username', width: 160 },
  { title: t('users.displayName'), dataIndex: 'displayName', key: 'displayName', width: 160 },
  { title: t('users.colDept'), dataIndex: 'deptName', key: 'deptName', width: 140 },
  { title: t('users.colPosts'), dataIndex: 'postNames', key: 'postNames', width: 160 },
  { title: t('users.email'), dataIndex: 'email', key: 'email' },
  { title: t('users.phone'), dataIndex: 'phone', key: 'phone', width: 140 },
  { title: t('users.colStatus'), dataIndex: 'status', key: 'status', width: 100 },
  { title: t('users.colType'), dataIndex: 'isSuper', key: 'isSuper', width: 90 },
  { title: t('users.colCreated'), dataIndex: 'createdAt', key: 'createdAt', width: 190 },
  { title: t('users.colActions'), key: 'actions', width: 170 },
]);

function statusText(status: UserVo['status']): string {
  return { active: t('users.statusActive'), locked: t('users.statusLocked'), disabled: t('users.statusDisabled') }[status] ?? status;
}

function statusColor(status: UserVo['status']): string {
  return { active: 'green', locked: 'orange', disabled: 'default' }[status] ?? 'default';
}

function reload(): void {
  void query.refetch();
}

function onSearch(): void {
  page.value = 1;
  keyword.value = searchText.value;
  void queryClient.invalidateQueries({ queryKey: ['users'] });
}

function onTableChange(pagination: TablePaginationConfig): void {
  page.value = pagination.current ?? 1;
  size.value = pagination.pageSize ?? 20;
}

function openCreate(): void {
  editingUser.value = null;
  createOpen.value = true;
}

function openEdit(record: UserVo): void {
  editingUser.value = record;
  createOpen.value = true;
}

function openRoles(record: UserVo): void {
  activeUser.value = record;
  roleOpen.value = true;
}

function openPosts(record: UserVo): void {
  activeUser.value = record;
  postOpen.value = true;
}

function toggleStatus(record: UserVo): void {
  const next = record.status === 'active' ? 'disabled' : 'active';
  void http
    .patch(`/api/system/users/${record.id}/status`, { status: next })
    .then(() => {
      message.success(t('users.statusUpdated'));
      reload();
    })
    .catch((e) => message.error(e instanceof ApiError ? e.message : t('users.saveFailed')));
}

function downloadTemplate(): void {
  window.open('/api/system/users/import-template', '_blank');
}

const unlockMutation = useMutation({
  mutationFn: (id: string) => http.post(`/api/system/users/${id}/unlock`),
  onSuccess: () => {
    message.success(t('users.unlocked'));
    void queryClient.invalidateQueries({ queryKey: ['users'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('users.unlockFailed'));
  },
});

function unlock(record: UserVo): void {
  unlockMutation.mutate(record.id);
}

/** 异步导出用户列表：创建任务 → 轮询 → 下载。 */
async function onExport(): Promise<void> {
  exporting.value = true;
  try {
    const task = await http.post<ExportTaskVo>('/api/system/export-tasks', {
      kind: 'system.users',
      keyword: keyword.value.trim() || null,
    });
    for (let i = 0; i < 60; i += 1) {
      await new Promise((resolve) => setTimeout(resolve, 500));
      const pageData = await http.get<{ data: ExportTaskVo[] }>(
        '/api/system/export-tasks?page=1&size=10',
      );
      const current = pageData.data.find((t) => t.id === task.id);
      if (current?.status === 'success') {
        window.open(`/api/system/export-tasks/${task.id}/download`, '_blank');
        message.success(t('users.exportDone'));
        return;
      }
      if (current?.status === 'failed') {
        throw new ApiError('export.failed', current.error ?? t('users.exportFailed'), 500);
      }
    }
    message.warning(t('users.exportPending'));
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('users.exportFailed'));
  } finally {
    exporting.value = false;
  }
}

/** 导入用户：multipart 上传，返回逐行结果。 */
async function onImport(options: UploadRequestOption): Promise<void> {
  const body = new FormData();
  body.append('file', options.file);

  importing.value = true;
  try {
    const response = await fetch(`/api/system/users/import?dryRun=${dryRun.value ? 'true' : 'false'}`, {
      method: 'POST',
      body,
      credentials: 'include',
    });
    const payload = await response.json().catch(() => ({}));
    if (!response.ok) {
      throw new ApiError(payload.code ?? 'import.failed', payload.detail ?? t('users.importFailed'), response.status);
    }
    importResult.value = payload.data as ImportResult;
    importOpen.value = true;
    options.onSuccess?.(payload);
    await reload();
  } catch (error) {
    const err = error instanceof ApiError ? error : new ApiError('import.failed', t('users.importFailed'), 0);
    message.error(err.message);
    options.onError?.(err as unknown as Error);
  } finally {
    importing.value = false;
  }
}

</script>

<style scoped>
.page { padding: 0; }
.card-title { display: flex; align-items: center; gap: 8px; }
.muted { color: #cbd5e1; }
</style>
