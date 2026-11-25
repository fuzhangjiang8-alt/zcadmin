<template>
  <div class="page">
    <a-card :bordered="false" :title="t('configs.title')">
      <template #extra>
        <a-space>
          <a-input-search
            v-model:value="searchText"
            :placeholder="t('configs.searchPlaceholder')"
            allow-clear
            style="width: 240px"
            @search="onSearch"
          />
          <a-button v-perm="PERMS.CONFIG_EDIT" type="primary" @click="openCreate">
            {{ t('configs.new') }}
          </a-button>
        </a-space>
      </template>

      <a-table
        :columns="columns"
        :data-source="configs"
        :loading="loading"
        row-key="id"
        :pagination="{
          current: page,
          pageSize: size,
          total,
          showSizeChanger: true,
          showTotal: (total: number) => t('configs.total', { n: total }),
        }"
        @change="onTableChange"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'isPublic'">
            <a-tag :color="record.isPublic ? 'blue' : 'default'">
              {{ record.isPublic ? t('configs.public') : t('configs.backendOnly') }}
            </a-tag>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-space>
              <a-button v-perm="PERMS.CONFIG_EDIT" type="link" size="small" @click="openEdit(record)">
                {{ t('configs.edit') }}
              </a-button>
              <a-popconfirm :title="t('configs.deleteConfirm')" @confirm="onDelete(record)">
                <a-button v-perm="PERMS.CONFIG_EDIT" type="link" danger size="small">
                  {{ t('configs.delete') }}
                </a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>

    <a-modal
      v-model:open="editOpen"
      :title="editing ? t('configs.editTitle', { key: editing.key }) : t('configs.newTitle')"
      :confirm-loading="submitting"
      @ok="submit"
    >
      <a-form ref="formRef" :model="form" :rules="rules" layout="vertical">
        <a-form-item v-if="!editing" :label="t('configs.key')" name="key">
          <a-input v-model:value="form.key" :placeholder="t('configs.keyPlaceholder')" />
        </a-form-item>
        <a-form-item :label="t('configs.name')" name="name">
          <a-input v-model:value="form.name" :placeholder="t('configs.namePlaceholder')" />
        </a-form-item>
        <a-form-item :label="t('configs.value')" name="value">
          <a-textarea v-model:value="form.value" :rows="2" />
        </a-form-item>
        <a-form-item :label="t('configs.remark')" name="remark">
          <a-input v-model:value="form.remark" :placeholder="t('configs.remarkPlaceholder')" />
        </a-form-item>
        <a-form-item :label="t('configs.publicLabel')" name="isPublic">
          <a-switch v-model:checked="form.isPublic" />
          <span class="hint">{{ t('configs.publicHint') }}</span>
        </a-form-item>
      </a-form>
    </a-modal>
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../permissions';
// =============================================================================
// 参数配置
//
// 键值对形式的系统参数；isPublic 控制是否通过 /api/system/configs/public 下发。
// 密钥类配置必须保持 isPublic = false。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { FormInstance, TablePaginationConfig } from 'ant-design-vue';
import { ApiError, http } from '../api/client';

interface ConfigVo {
  id: string;
  key: string;
  value: string;
  name: string;
  remark: string | null;
  isPublic: boolean;
  updatedAt: string | null;
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
  queryKey: computed(() => ['configs', page.value, size.value, keyword.value] as const),
  queryFn: async () => {
    const params = new URLSearchParams({ page: String(page.value), size: String(size.value) });
    if (keyword.value.trim()) params.set('keyword', keyword.value.trim());
    return http.get<PageResult<ConfigVo>>(`/api/system/configs?${params.toString()}`);
  },
  placeholderData: keepPreviousData,
});

const configs = computed(() => query.data.value?.data ?? []);
const total = computed(() => query.data.value?.page.total ?? 0);
const loading = computed(() => query.isFetching.value);

watch(query.error, (error) => {
  if (error) {
    message.error(error instanceof ApiError ? error.message : t('configs.loadFailed'));
  }
});

const columns = computed(() => [
  { title: t('configs.colKey'), dataIndex: 'key', key: 'key', width: 220 },
  { title: t('configs.colName'), dataIndex: 'name', key: 'name', width: 180 },
  { title: t('configs.colValue'), dataIndex: 'value', key: 'value' },
  { title: t('configs.colVisible'), dataIndex: 'isPublic', key: 'isPublic', width: 110 },
  { title: t('configs.colUpdated'), dataIndex: 'updatedAt', key: 'updatedAt', width: 190 },
  { title: t('configs.colActions'), key: 'actions', width: 90 },
]);

function onSearch(): void {
  page.value = 1;
  keyword.value = searchText.value;
  void queryClient.invalidateQueries({ queryKey: ['configs'] });
}

function onTableChange(pagination: TablePaginationConfig): void {
  page.value = pagination.current ?? 1;
  size.value = pagination.pageSize ?? 20;
}

const editOpen = ref(false);
const submitting = ref(false);
const editing = ref<ConfigVo | null>(null);
const formRef = ref<FormInstance>();
const form = reactive({ key: '', name: '', value: '', remark: '', isPublic: false });
const rules = {
  key: [{ required: true, max: 128, message: t('configs.keyRule') }],
  name: [{ required: true, message: t('configs.nameRequired') }],
  value: [{ required: true, message: t('configs.valueRequired') }],
};

function openCreate(): void {
  editing.value = null;
  form.key = '';
  form.name = '';
  form.value = '';
  form.remark = '';
  form.isPublic = false;
  editOpen.value = true;
}

function openEdit(vo: ConfigVo): void {
  editing.value = vo;
  form.key = vo.key;
  form.name = vo.name;
  form.value = vo.value;
  form.remark = vo.remark ?? '';
  form.isPublic = vo.isPublic;
  editOpen.value = true;
}

const saveMutation = useMutation({
  mutationFn: async () => {
    if (editing.value) {
      return http.put<ConfigVo>(`/api/system/configs/${editing.value.id}`, {
        value: form.value,
        name: form.name.trim(),
        remark: form.remark.trim() || null,
        isPublic: form.isPublic,
      });
    }
    return http.post<ConfigVo>('/api/system/configs', {
      key: form.key.trim(),
      value: form.value,
      name: form.name.trim(),
      remark: form.remark.trim() || null,
      isPublic: form.isPublic,
    });
  },
  onSuccess: () => {
    message.success(t('configs.saved'));
    editOpen.value = false;
    void queryClient.invalidateQueries({ queryKey: ['configs'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('configs.saveFailed'));
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
  mutationFn: (id: string) => http.delete(`/api/system/configs/${id}`),
  onSuccess: () => {
    message.success(t('configs.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['configs'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('configs.deleteFailed'));
  },
});

function onDelete(vo: ConfigVo): void {
  deleteMutation.mutate(vo.id);
}
</script>

<style scoped>
.page {
  padding: 0;
}

.hint {
  margin-left: 8px;
  color: #94a3b8;
  font-size: 12px;
}
</style>
