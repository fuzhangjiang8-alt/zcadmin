<template>
  <div class="page">
    <a-card :bordered="false" :title="t('crm.contactsTitle')">
      <template #extra>
        <a-space>
          <a-input-search
            v-model:value="searchText"
            :placeholder="t('crm.searchContact')"
            allow-clear
            style="width: 240px"
            @search="onSearch"
          />
          <a-button v-perm="PERMS.CONTACT_LIST" :loading="exporting" @click="exportNow">{{ t('crm.exportBtn') }}</a-button>
          <a-upload :before-upload="onImport" :show-upload-list="false" accept=".xlsx">
            <a-button v-perm="PERMS.CONTACT_ADD" :loading="importing">{{ t('crm.importBtn') }}</a-button>
          </a-upload>
          <a-button v-perm="PERMS.CONTACT_ADD" type="primary" @click="openCreate">
            {{ t('crm.newContact') }}
          </a-button>
        </a-space>
      </template>

      <a-table
        :columns="columns"
        :data-source="contacts"
        :loading="loading"
        row-key="id"
        :pagination="{
          current: page,
          pageSize: size,
          total,
          showSizeChanger: true,
          showTotal: (total: number) => t('crm.totalContacts', { n: total }),
        }"
        @change="onTableChange"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'customer'">{{ customerName(record.customerId) }}</template>
          <template v-else-if="column.key === 'isPrimary'">
            <a-tag v-if="record.isPrimary" color="blue">{{ t('crm.primary') }}</a-tag>
            <span v-else class="muted">—</span>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-space>
              <a-button v-perm="PERMS.CONTACT_EDIT" type="link" size="small" @click="openEdit(record)">
                {{ t('crm.edit') }}
              </a-button>
              <a-popconfirm :title="t('crm.deleteConfirmContact')" @confirm="onDelete(record)">
                <a-button v-perm="PERMS.CONTACT_DELETE" type="link" size="small" danger>
                  {{ t('crm.delete') }}
                </a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>

    <a-modal
      v-model:open="editOpen"
      :title="editing ? t('crm.editContact') : t('crm.newContact')"
      :confirm-loading="submitting"
      @ok="submit"
    >
      <a-form ref="formRef" :model="form" :rules="rules" layout="vertical">
        <a-form-item :label="t('crm.customer')" name="customerId">
          <a-select
            v-model:value="form.customerId"
            :options="customerOptions"
            :disabled="!!editing"
            show-search
            option-filter-prop="label"
            :placeholder="t('crm.customerPlaceholder')"
          />
        </a-form-item>
        <a-form-item :label="t('crm.contactName')" name="name">
          <a-input v-model:value="form.name" :placeholder="t('crm.contactNamePlaceholder')" />
        </a-form-item>
        <a-form-item :label="t('crm.phone')" name="phone">
          <a-input v-model:value="form.phone" :placeholder="t('crm.optional')" />
        </a-form-item>
        <a-form-item :label="t('crm.email')" name="email">
          <a-input v-model:value="form.email" :placeholder="t('crm.optional')" />
        </a-form-item>
        <a-form-item :label="t('crm.contactPrimary')" name="isPrimary">
          <a-switch v-model:checked="form.isPrimary" />
        </a-form-item>
      </a-form>
    </a-modal>
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../../permissions';
// =============================================================================
// CRM 联系人
//
// 联系人必须挂在客户下（customer_id 外键）；编辑时不允许改所属客户，
// 避免把联系人"搬"到别的客户导致历史记录错乱。
// =============================================================================

import { computed, onMounted, reactive, ref, watch } from 'vue';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import type { FormInstance, TablePaginationConfig } from 'ant-design-vue';
import { ApiError, http } from '../../api/client';
import { useImport } from './useImport';
import { useExport } from './useExport';

interface ContactVo {
  id: string;
  customerId: string;
  name: string;
  phone: string | null;
  email: string | null;
  isPrimary: boolean;
}

interface CustomerVo {
  id: string;
  name: string;
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
const customers = ref<CustomerVo[]>([]);

const query = useQuery({
  queryKey: computed(() => ['crm-contacts', page.value, size.value, keyword.value] as const),
  queryFn: () => {
    const params = new URLSearchParams({ page: String(page.value), size: String(size.value) });
    if (keyword.value.trim()) params.set('keyword', keyword.value.trim());
    return http.get<PageResult<ContactVo>>(`/api/crm/contacts?${params.toString()}`);
  },
  placeholderData: keepPreviousData,
});
const contacts = computed(() => query.data.value?.data ?? []);
const total = computed(() => query.data.value?.page.total ?? 0);
const loading = computed(() => query.isFetching.value);

watch(query.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('crm.loadFailed'));
});

const { exporting, exportNow } = useExport('crm.contacts');
const { importing, importFile } = useImport('crm.contacts');

function onSearch(): void {
  page.value = 1;
  keyword.value = searchText.value;
  void queryClient.invalidateQueries({ queryKey: ['crm-contacts'] });
}

function onImport(file: File): boolean {
  void importFile(file).then((ok) => {
    if (ok) void queryClient.invalidateQueries({ queryKey: ['crm-contacts'] });
  });
  return false;
}

const columns = computed(() => [
  { title: t('crm.colName'), dataIndex: 'name', key: 'name', width: 150 },
  { title: t('crm.colCustomer'), dataIndex: 'customerId', key: 'customer', width: 200 },
  { title: t('crm.colPhone'), dataIndex: 'phone', key: 'phone', width: 160 },
  { title: t('crm.email'), dataIndex: 'email', key: 'email' },
  { title: t('crm.colPrimary'), dataIndex: 'isPrimary', key: 'isPrimary', width: 90 },
  { title: t('crm.colActions'), key: 'actions', width: 150 },
]);

const customerOptions = computed(() =>
  customers.value.map((c) => ({ label: c.name, value: c.id })),
);

function customerName(id: string): string {
  return customers.value.find((c) => c.id === id)?.name ?? id.slice(0, 8);
}

async function loadCustomers(): Promise<void> {
  if (customers.value.length > 0) return;
  const result = await http.get<PageResult<CustomerVo>>('/api/crm/customers?page=1&size=200');
  customers.value = result.data;
}

function onTableChange(pagination: TablePaginationConfig): void {
  page.value = pagination.current ?? 1;
  size.value = pagination.pageSize ?? 20;
}

// ---- 新建 / 编辑 ----
const editOpen = ref(false);
const submitting = ref(false);
const editing = ref<ContactVo | null>(null);
const formRef = ref<FormInstance>();
const form = reactive({
  customerId: undefined as string | undefined,
  name: '',
  phone: '',
  email: '',
  isPrimary: false,
});
const rules = {
  customerId: [{ required: true, message: t('crm.selectCustomer') }],
  name: [{ required: true, max: 64, message: t('crm.contactNameRule') }],
};

async function openCreate(): Promise<void> {
  editing.value = null;
  Object.assign(form, { customerId: undefined, name: '', phone: '', email: '', isPrimary: false });
  editOpen.value = true;
  try {
    await loadCustomers();
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('crm.loadCustomerFailed'));
  }
}

async function openEdit(record: ContactVo): Promise<void> {
  editing.value = record;
  Object.assign(form, {
    customerId: record.customerId,
    name: record.name,
    phone: record.phone ?? '',
    email: record.email ?? '',
    isPrimary: record.isPrimary,
  });
  editOpen.value = true;
  void loadCustomers().catch(() => undefined);
}

async function submit(): Promise<void> {
  try {
    await formRef.value?.validate();
  } catch {
    return;
  }
  submitting.value = true;
  try {
    if (editing.value) {
      await http.patch<ContactVo>(`/api/crm/contacts/${editing.value.id}`, {
        name: form.name.trim(),
        phone: form.phone.trim() || null,
        email: form.email.trim() || null,
        isPrimary: form.isPrimary,
      });
    } else {
      await http.post<ContactVo>('/api/crm/contacts', {
        customerId: form.customerId,
        name: form.name.trim(),
        phone: form.phone.trim() || null,
        email: form.email.trim() || null,
        isPrimary: form.isPrimary,
      });
    }
    message.success(t('crm.saved'));
    editOpen.value = false;
    void queryClient.invalidateQueries({ queryKey: ['crm-contacts'] });
  } catch (error) {
    message.error(error instanceof ApiError ? error.message : t('crm.saveFailed'));
  } finally {
    submitting.value = false;
  }
}

const deleteMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/crm/contacts/${id}`),
  onSuccess: () => {
    message.success(t('crm.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['crm-contacts'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('crm.deleteFailed'));
  },
});

function onDelete(record: ContactVo): void {
  deleteMutation.mutate(record.id);
}

onMounted(() => {
  void loadCustomers().catch(() => undefined);
});
</script>

<style scoped>
.page { padding: 0; }
.muted { color: #cbd5e1; }
</style>
