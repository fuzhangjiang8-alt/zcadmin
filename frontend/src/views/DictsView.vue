<template>
  <div class="page">
    <a-card :bordered="false" :title="t('dicts.title')">
      <template #extra>
        <a-button v-perm="PERMS.DICT_ADD" type="primary" @click="openCreateType">
          {{ t('dicts.new') }}
        </a-button>
      </template>

      <a-table :columns="typeColumns" :data-source="dictTypes" :loading="loading" row-key="id">
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'itemCount'">
            <a-tag>{{ t('dicts.itemsCount', { n: record.itemCount }) }}</a-tag>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-button type="link" size="small" @click="openItems(record)">{{ t('dicts.items') }}</a-button>
            <a-button v-perm="PERMS.DICT_EDIT" type="link" size="small" @click="openEditType(record)">{{ t('dicts.edit') }}</a-button>
            <a-popconfirm :title="t('dicts.deleteTypeConfirm')" @confirm="deleteType(record)">
              <a-button v-perm="PERMS.DICT_EDIT" type="link" danger size="small">{{ t('dicts.delete') }}</a-button>
            </a-popconfirm>
          </template>
        </template>
      </a-table>
    </a-card>

    <!-- 新建字典类型 -->
    <a-modal
      v-model:open="typeOpen"
      :title="editingType ? t('dicts.editTitle') : t('dicts.createTitle')"
      :confirm-loading="typeSaving"
      @ok="submitType"
    >
      <a-form ref="typeFormRef" :model="typeForm" :rules="typeRules" layout="vertical">
        <a-form-item :label="t('dicts.code')" name="code">
          <a-input v-model:value="typeForm.code" :placeholder="t('dicts.codePlaceholder')" :disabled="!!editingType" />
        </a-form-item>
        <a-form-item :label="t('dicts.name')" name="name">
          <a-input v-model:value="typeForm.name" :placeholder="t('dicts.namePlaceholder')" />
        </a-form-item>
        <a-form-item :label="t('dicts.remark')" name="remark">
          <a-textarea v-model:value="typeForm.remark" :rows="2" />
        </a-form-item>
      </a-form>
    </a-modal>

    <!-- 字典项 -->
    <a-drawer v-model:open="itemOpen" :title="t('dicts.drawerTitle', { name: activeType?.name ?? '' })" width="620">
      <a-form layout="inline" :model="itemForm" class="item-form">
        <a-form-item :label="t('dicts.label')"><a-input v-model:value="itemForm.label" :placeholder="t('dicts.labelPlaceholder')" style="width: 140px" /></a-form-item>
        <a-form-item :label="t('dicts.value')"><a-input v-model:value="itemForm.value" :placeholder="t('dicts.valuePlaceholder')" style="width: 140px" /></a-form-item>
        <a-form-item :label="t('dicts.sort')"><a-input-number v-model:value="itemForm.sort" :min="0" style="width: 90px" /></a-form-item>
        <a-form-item>
          <a-button v-perm="PERMS.DICT_ADD" type="primary" :loading="itemSaving" @click="submitItem">{{ t('dicts.add') }}</a-button>
        </a-form-item>
      </a-form>

      <a-table :columns="itemColumns" :data-source="items" :loading="itemLoading" row-key="id" size="small" :pagination="false">
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'actions'">
            <a-space>
              <a-button v-perm="PERMS.DICT_EDIT" type="link" size="small" @click="openEditItem(record)">{{ t('dicts.edit') }}</a-button>
              <a-popconfirm :title="t('dicts.deleteConfirm')" @confirm="deleteItem(record)">
                <a-button v-perm="PERMS.DICT_EDIT" type="link" danger size="small">{{ t('dicts.delete') }}</a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-drawer>

    <DictItemEditModal
      v-model:open="itemEditOpen"
      :code="activeType?.code ?? ''"
      :item="editingItem"
      @saved="loadItems"
    />
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../permissions';
// =============================================================================
// 字典管理
//
// 价值：把枚举值从代码里搬到数据库，运营可维护。
// 用法：前端通过 useDict('code') 取到 [{label,value}]，后端校验只认 value。
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { FormInstance } from 'ant-design-vue';
import { ApiError, http } from '../api/client';
import DictItemEditModal from '../features/dicts/DictItemEditModal.vue';

interface DictTypeVo {
  id: string;
  code: string;
  name: string;
  remark: string | null;
  status: string;
  itemCount: number;
}

interface DictItemVo {
  id: string;
  label: string;
  value: string;
  sort: number;
  status: string;
}

const { t } = useI18n();
const queryClient = useQueryClient();
const query = useQuery({
  queryKey: ['dicts'],
  queryFn: () => http.get<DictTypeVo[]>('/api/system/dicts'),
});
const dictTypes = computed(() => query.data.value ?? []);
const loading = computed(() => query.isFetching.value);
watch(query.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('dicts.loadFailed'));
});

const typeColumns = computed(() => [
  { title: t('dicts.colCode'), dataIndex: 'code', key: 'code', width: 200 },
  { title: t('dicts.colName'), dataIndex: 'name', key: 'name', width: 180 },
  { title: t('dicts.colRemark'), dataIndex: 'remark', key: 'remark' },
  { title: t('dicts.colCount'), dataIndex: 'itemCount', key: 'itemCount', width: 100 },
  { title: t('dicts.colActions'), key: 'actions', width: 110 },
]);

// ---- 新建 / 编辑字典类型 ----
const typeOpen = ref(false);
const editingType = ref<DictTypeVo | null>(null);
const typeSaving = ref(false);
const typeFormRef = ref<FormInstance>();
const typeForm = reactive({ code: '', name: '', remark: '' });
const typeRules = {
  code: [{ required: true, min: 2, max: 64, message: t('dicts.codeRule') }],
  name: [{ required: true, message: t('dicts.nameRequired') }],
};

function openEditType(record: DictTypeVo): void {
  editingType.value = record;
  typeForm.code = record.code;
  typeForm.name = record.name;
  typeForm.remark = record.remark ?? '';
  typeOpen.value = true;
}

function openCreateType(): void {
  editingType.value = null;
  Object.assign(typeForm, { code: '', name: '', remark: '' });
  typeOpen.value = true;
}

async function deleteType(row: DictTypeVo): Promise<void> {
  try {
    await http.delete(`/api/system/dicts/${row.code}`);
    message.success(t('dicts.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['dicts'] });
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('dicts.deleteFailed'));
  }
}

async function submitType(): Promise<void> {
  try {
    await typeFormRef.value?.validate();
  } catch {
    return;
  }
  typeSaving.value = true;
  try {
    if (editingType.value) {
      await http.put<DictTypeVo>(`/api/system/dicts/${editingType.value.code}`, {
        name: typeForm.name.trim(),
        remark: typeForm.remark.trim() || null,
      });
      message.success(t('dicts.saved'));
    } else {
      await http.post<DictTypeVo>('/api/system/dicts', {
        code: typeForm.code.trim(),
        name: typeForm.name.trim(),
        remark: typeForm.remark.trim() || null,
      });
      message.success(t('dicts.created'));
    }
    typeOpen.value = false;
    void queryClient.invalidateQueries({ queryKey: ['dicts'] });
  } catch (error) {
    message.error(
      error instanceof ApiError
        ? error.message
        : editingType.value
          ? t('dicts.saveFailed')
          : t('dicts.createFailed'),
    );
  } finally {
    typeSaving.value = false;
  }
}

// ---- 字典项 ----
const itemOpen = ref(false);
const itemLoading = ref(false);
const itemSaving = ref(false);
const activeType = ref<DictTypeVo | null>(null);
const items = ref<DictItemVo[]>([]);
const itemForm = reactive({ label: '', value: '', sort: 0 });
const itemEditOpen = ref(false);
const editingItem = ref<DictItemVo | null>(null);

const itemColumns = computed(() => [
  { title: t('dicts.colLabel'), dataIndex: 'label', key: 'label' },
  { title: t('dicts.colValue'), dataIndex: 'value', key: 'value' },
  { title: t('dicts.colSort'), dataIndex: 'sort', key: 'sort', width: 80 },
  { title: t('dicts.colActions'), key: 'actions', width: 120 },
]);

async function openItems(type: DictTypeVo): Promise<void> {
  activeType.value = type;
  itemOpen.value = true;
  await loadItems();
}

async function loadItems(): Promise<void> {
  if (!activeType.value) return;
  itemLoading.value = true;
  try {
    items.value = await http.get<DictItemVo[]>(
      `/api/system/dicts/${activeType.value.code}/items`,
    );
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('dicts.itemLoadFailed'));
  } finally {
    itemLoading.value = false;
  }
}

function openEditItem(item: DictItemVo): void {
  editingItem.value = item;
  itemEditOpen.value = true;
}

async function deleteItem(item: DictItemVo): Promise<void> {
  if (!activeType.value) return;
  try {
    await http.delete(`/api/system/dicts/${activeType.value.code}/items/${item.id}`);
    message.success(t('dicts.deleted'));
    await loadItems();
    void queryClient.invalidateQueries({ queryKey: ['dicts'] });
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('dicts.deleteFailed'));
  }
}

async function submitItem(): Promise<void> {
  if (!activeType.value) return;
  if (!itemForm.label.trim() || !itemForm.value.trim()) {
    message.warning(t('dicts.labelValueRequired'));
    return;
  }
  itemSaving.value = true;
  try {
    await http.post<DictItemVo>(`/api/system/dicts/${activeType.value.code}/items`, {
      label: itemForm.label.trim(),
      value: itemForm.value.trim(),
      sort: itemForm.sort,
    });
    message.success(t('dicts.added'));
    itemForm.label = '';
    itemForm.value = '';
    itemForm.sort = 0;
    await loadItems();
    void queryClient.invalidateQueries({ queryKey: ['dicts'] });
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('dicts.addFailed'));
  } finally {
    itemSaving.value = false;
  }
}

</script>

<style scoped>
.page {
  padding: 0;
}

.item-form {
  margin-bottom: 16px;
}
</style>
