<template>
  <div class="page">
    <a-card :bordered="false" :title="t('posts.title')">
      <template #extra>
        <a-space>
          <a-input-search
            v-model:value="searchText"
            :placeholder="t('posts.searchPlaceholder')"
            allow-clear
            style="width: 240px"
            @search="onSearch"
          />
          <a-button v-perm="PERMS.POST_ADD" type="primary" @click="openCreate">
            {{ t('posts.new') }}
          </a-button>
        </a-space>
      </template>

      <a-table
        :columns="columns"
        :data-source="posts"
        :loading="loading"
        row-key="id"
        :pagination="{
          current: page,
          pageSize: size,
          total,
          showSizeChanger: true,
          showTotal: (total: number) => t('posts.total', { n: total }),
        }"
        @change="onTableChange"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'status'">
            <a-tag :color="record.status === 'active' ? 'green' : 'default'">
              {{ record.status === 'active' ? t('posts.active') : t('posts.disabled') }}
            </a-tag>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-space>
              <a-button v-perm="PERMS.POST_EDIT" type="link" size="small" @click="openEdit(record)">
                {{ t('posts.edit') }}
              </a-button>
              <a-popconfirm :title="t('posts.deleteConfirm')" @confirm="onDelete(record)">
                <a-button v-perm="PERMS.POST_DELETE" type="link" size="small" danger>
                  {{ t('posts.delete') }}
                </a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>

    <a-modal
      v-model:open="editOpen"
      :title="editing ? t('posts.editTitle') : t('posts.newTitle')"
      :confirm-loading="submitting"
      @ok="submit"
    >
      <a-form ref="formRef" :model="form" :rules="rules" layout="vertical">
        <a-form-item :label="t('posts.code')" name="code">
          <a-input v-model:value="form.code" :placeholder="t('posts.codePlaceholder')" />
        </a-form-item>
        <a-form-item :label="t('posts.name')" name="name">
          <a-input v-model:value="form.name" :placeholder="t('posts.namePlaceholder')" />
        </a-form-item>
        <a-form-item :label="t('posts.sort')" name="sort">
          <a-input-number v-model:value="form.sort" :min="0" :max="9999" style="width: 160px" />
        </a-form-item>
        <a-form-item :label="t('posts.status')" name="status">
          <a-select v-model:value="form.status" :options="statusOptions" />
        </a-form-item>
        <a-form-item :label="t('posts.remark')" name="remark">
          <a-textarea v-model:value="form.remark" :rows="2" />
        </a-form-item>
      </a-form>
    </a-modal>
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../permissions';
// =============================================================================
// 岗位管理
//
// 岗位是职责标签，与部门（组织归属）正交；用户可绑定多个岗位。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { FormInstance, TablePaginationConfig } from 'ant-design-vue';
import { ApiError, http } from '../api/client';

interface PostVo {
  id: string;
  code: string;
  name: string;
  sort: number;
  status: string;
  remark: string | null;
}

interface PageResult<T> {
  data: T[];
  page: { page: number; size: number; total: number; hasNext: boolean };
}

const { t } = useI18n();
const queryClient = useQueryClient();
const searchText = ref('');
const keyword = ref('');
const page = ref(1);
const size = ref(20);

const query = useQuery({
  queryKey: computed(() => ['posts', page.value, size.value, keyword.value] as const),
  queryFn: async () => {
    const params = new URLSearchParams({ page: String(page.value), size: String(size.value) });
    if (keyword.value.trim()) params.set('keyword', keyword.value.trim());
    return http.get<PageResult<PostVo>>(`/api/system/posts?${params.toString()}`);
  },
  placeholderData: keepPreviousData,
});

const posts = computed(() => query.data.value?.data ?? []);
const total = computed(() => query.data.value?.page.total ?? 0);
const loading = computed(() => query.isFetching.value);

watch(query.error, (error) => {
  if (error) {
    message.error(error instanceof ApiError ? error.message : t('posts.loadFailed'));
  }
});

const statusOptions = computed(() => [
  { label: t('posts.active'), value: 'active' },
  { label: t('posts.disabled'), value: 'disabled' },
]);

const columns = computed(() => [
  { title: t('posts.colCode'), dataIndex: 'code', key: 'code', width: 180 },
  { title: t('posts.colName'), dataIndex: 'name', key: 'name', width: 180 },
  { title: t('posts.colSort'), dataIndex: 'sort', key: 'sort', width: 80 },
  { title: t('posts.colStatus'), dataIndex: 'status', key: 'status', width: 100 },
  { title: t('posts.colRemark'), dataIndex: 'remark', key: 'remark' },
  { title: t('posts.colActions'), key: 'actions', width: 150 },
]);

function onSearch(): void {
  page.value = 1;
  keyword.value = searchText.value;
  void queryClient.invalidateQueries({ queryKey: ['posts'] });
}

function onTableChange(pagination: TablePaginationConfig): void {
  page.value = pagination.current ?? 1;
  size.value = pagination.pageSize ?? 20;
}

const editOpen = ref(false);
const submitting = ref(false);
const editing = ref<PostVo | null>(null);
const formRef = ref<FormInstance>();
const form = reactive({ code: '', name: '', sort: 0, status: 'active', remark: '' });
const rules = {
  code: [{ required: true, max: 64, message: t('posts.codeRule') }],
  name: [{ required: true, max: 64, message: t('posts.nameRule') }],
};

function openCreate(): void {
  editing.value = null;
  form.code = '';
  form.name = '';
  form.sort = 0;
  form.status = 'active';
  form.remark = '';
  editOpen.value = true;
}

function openEdit(vo: PostVo): void {
  editing.value = vo;
  form.code = vo.code;
  form.name = vo.name;
  form.sort = vo.sort;
  form.status = vo.status;
  form.remark = vo.remark ?? '';
  editOpen.value = true;
}

const saveMutation = useMutation({
  mutationFn: async () => {
    const payload = {
      code: form.code.trim(),
      name: form.name.trim(),
      sort: form.sort,
      status: form.status,
      remark: form.remark.trim() || null,
    };
    if (editing.value) {
      return http.put<PostVo>(`/api/system/posts/${editing.value.id}`, payload);
    }
    return http.post<PostVo>('/api/system/posts', payload);
  },
  onSuccess: () => {
    message.success(t('posts.saved'));
    editOpen.value = false;
    void queryClient.invalidateQueries({ queryKey: ['posts'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('posts.saveFailed'));
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
  mutationFn: (id: string) => http.delete(`/api/system/posts/${id}`),
  onSuccess: () => {
    message.success(t('posts.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['posts'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('posts.deleteFailed'));
  },
});

function onDelete(vo: PostVo): void {
  deleteMutation.mutate(vo.id);
}
</script>

<style scoped>
.page {
  padding: 0;
}
</style>
