<template>
  <div class="page">
    <a-card :bordered="false" :title="t('tenants.title')">
      <template #extra>
        <a-space>
          <a-switch
            v-model:checked="showDeleted"
            :checked-children="t('tenants.includeDeleted')"
            :un-checked-children="t('tenants.onlyActive')"
            @change="onToggleDeleted"
          />
          <a-button v-perm="PERMS.TENANT_LIST" @click="billsOpen = true">{{ t('tenants.bills') }}</a-button>
          <a-button v-perm="PERMS.TENANT_LIST" @click="reconOpen = true">{{ t('tenants.recon') }}</a-button>
          <a-button v-perm="PERMS.TENANT_LIST" @click="exportBills">{{ t('tenants.exportBills') }}</a-button>
          <a-button v-perm="PERMS.TENANT_LIST" @click="exportBillsXlsx">{{ t('tenants.exportExcel') }}</a-button>
          <a-popconfirm
            :title="t('tenants.purgeConfirm')"
            @confirm="purgeTenants"
          >
            <a-button v-perm="PERMS.TENANT_DELETE" danger>{{ t('tenants.purge') }}</a-button>
          </a-popconfirm>
          <a-button v-perm="PERMS.TENANT_ADD" type="primary" @click="createOpen = true">
            {{ t('tenants.new') }}
          </a-button>
        </a-space>
      </template>

      <a-table :data-source="tenants" :columns="columns" :loading="loading" row-key="id">
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'status'">
            <a-tag :color="record.status === 'active' ? 'green' : record.status === 'suspended' ? 'red' : 'default'">
              {{ record.status === 'active' ? t('tenants.statusActive') : record.status === 'suspended' ? t('tenants.statusSuspended') : t('tenants.statusDeleted') }}
            </a-tag>
          </template>
          <template v-else-if="column.key === 'userCount'">
            <a-progress
              v-if="record.maxUsers > 0"
              :percent="percent(record.userCount, record.maxUsers)"
              size="small"
              :format="() => `${record.userCount}/${record.maxUsers}`"
            />
            <span v-else>{{ record.userCount }} / {{ t('tenants.unlimited') }}</span>
          </template>
          <template v-else-if="column.key === 'storageUsed'">
            <a-progress
              v-if="record.maxStorageBytes > 0"
              :percent="percent(record.storageUsed, record.maxStorageBytes)"
              size="small"
              :format="() => storageText(record)"
            />
            <span v-else>{{ storageText(record) }} / {{ t('tenants.unlimited') }}</span>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-button
              v-if="record.status === 'deleted'"
              v-perm="PERMS.TENANT_EDIT"
              type="link"
              size="small"
              @click="restoreTenant(record)"
            >
              {{ t('tenants.restore') }}
            </a-button>
            <template v-else>
              <a-button v-perm="PERMS.TENANT_LIST" type="link" size="small" @click="openDetail(record)">
                {{ t('tenants.detail') }}
              </a-button>
              <a-button v-perm="PERMS.TENANT_LIST" type="link" size="small" @click="quickLogin(record)">
                {{ t('tenants.login') }}
              </a-button>
              <a-button v-perm="PERMS.TENANT_EDIT" type="link" size="small" @click="openBilling(record)">
                {{ t('tenants.billing') }}
              </a-button>
              <a-button v-perm="PERMS.TENANT_EDIT" type="link" size="small" @click="openQuota(record)">
                {{ t('tenants.quota') }}
              </a-button>
              <a-popconfirm :title="t('tenants.deleteConfirm')" @confirm="deleteTenant(record)">
                <a-button v-perm="PERMS.TENANT_DELETE" type="link" size="small" danger>{{ t('tenants.delete') }}</a-button>
              </a-popconfirm>
              <a-popconfirm
                :title="record.status === 'active' ? t('tenants.disableConfirm') : t('tenants.enableConfirm')"
                @confirm="toggleStatus(record)"
              >
                <a-button
                  v-perm="PERMS.TENANT_EDIT"
                  type="link"
                  size="small"
                  :danger="record.status === 'active'"
                >
                  {{ record.status === 'active' ? t('tenants.disable') : t('tenants.enable') }}
                </a-button>
              </a-popconfirm>
            </template>
          </template>
        </template>
      </a-table>
    </a-card>

    <TenantCreateModal v-model:open="createOpen" @saved="reload" />
    <TenantDetailModal v-model:open="detailOpen" :tenant="activeTenant" />
    <TenantBillsModal v-model:open="billsOpen" />
    <TenantReconModal v-model:open="reconOpen" />
    <TenantBillingModal v-model:open="billingOpen" :tenant="activeTenant" @saved="reload" />
    <TenantQuotaModal v-model:open="quotaOpen" :tenant="activeTenant" @saved="reload" />
  </div>
</template>

<script setup lang="ts">
// =============================================================================
// 租户管理页
//
// 页面只保留列表、工具栏与动作编排；创建/详情/账单/对账/计费/配额
// 拆到 features/tenants，每个弹窗自己负责加载与保存。
// =============================================================================

import { computed, ref, watch } from 'vue';
import { useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../api/client';
import { PERMS } from '../permissions';
import TenantBillsModal from '../features/tenants/TenantBillsModal.vue';
import TenantBillingModal from '../features/tenants/TenantBillingModal.vue';
import TenantCreateModal from '../features/tenants/TenantCreateModal.vue';
import TenantDetailModal from '../features/tenants/TenantDetailModal.vue';
import TenantQuotaModal from '../features/tenants/TenantQuotaModal.vue';
import TenantReconModal from '../features/tenants/TenantReconModal.vue';
import { storageText } from '../features/tenants/types';
import type { TenantVo } from '../features/tenants/types';

const { t } = useI18n();
const queryClient = useQueryClient();
const showDeleted = ref(false);

const query = useQuery({
  queryKey: computed(() => ['tenants', showDeleted.value] as const),
  queryFn: () =>
    http.get<TenantVo[]>(`/api/system/tenants${showDeleted.value ? '?includeDeleted=true' : ''}`),
});
const tenants = computed(() => query.data.value ?? []);
const loading = computed(() => query.isFetching.value);

watch(query.error, (error) => {
  if (error) {
    message.error(error instanceof ApiError ? error.message : t('tenants.loadFailed'));
  }
});

const createOpen = ref(false);
const detailOpen = ref(false);
const billsOpen = ref(false);
const reconOpen = ref(false);
const billingOpen = ref(false);
const quotaOpen = ref(false);
const activeTenant = ref<TenantVo | null>(null);

const columns = computed(() => [
  { title: t('tenants.colCode'), dataIndex: 'code', key: 'code', width: 160 },
  { title: t('tenants.colName'), dataIndex: 'name', key: 'name' },
  { title: t('tenants.colStatus'), dataIndex: 'status', key: 'status', width: 100 },
  { title: t('tenants.colUsers'), key: 'userCount', width: 190 },
  { title: t('tenants.colStorage'), key: 'storageUsed', width: 190 },
  { title: t('tenants.colCreated'), dataIndex: 'createdAt', key: 'createdAt', width: 190 },
  { title: t('tenants.colActions'), key: 'actions', width: 100 },
]);

function percent(used: number, max: number): number {
  return max > 0 ? Math.min(100, Math.round((used / max) * 100)) : 0;
}

function onToggleDeleted(): void {
  void queryClient.invalidateQueries({ queryKey: ['tenants'] });
}

function reload(): void {
  void query.refetch();
}

function openDetail(tenant: TenantVo): void {
  activeTenant.value = tenant;
  detailOpen.value = true;
}

function openBilling(tenant: TenantVo): void {
  activeTenant.value = tenant;
  billingOpen.value = true;
}

function openQuota(tenant: TenantVo): void {
  activeTenant.value = tenant;
  quotaOpen.value = true;
}

function exportBills(): void {
  window.open('/api/system/tenants/bills/export', '_blank');
}

function exportBillsXlsx(): void {
  window.open('/api/system/tenants/bills/export.xlsx', '_blank');
}

const purgeMutation = useMutation({
  mutationFn: () => http.post<{ purged: number }>('/api/system/tenants/purge?retentionDays=30'),
  onSuccess: (result) => {
    message.success(t('tenants.purged', { n: result.purged }));
    void queryClient.invalidateQueries({ queryKey: ['tenants'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('tenants.purgeFailed'));
  },
});

function purgeTenants(): void {
  purgeMutation.mutate();
}

function quickLogin(tenant: TenantVo): void {
  window.open(`/login?tenant=${encodeURIComponent(tenant.code)}`, '_blank');
}

const restoreMutation = useMutation({
  mutationFn: (id: string) => http.post(`/api/system/tenants/${id}/restore`),
  onSuccess: () => {
    message.success(t('tenants.restored'));
    void queryClient.invalidateQueries({ queryKey: ['tenants'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('tenants.restoreFailed'));
  },
});

function restoreTenant(tenant: TenantVo): void {
  restoreMutation.mutate(tenant.id);
}

const deleteMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/system/tenants/${id}`),
  onSuccess: () => {
    message.success(t('tenants.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['tenants'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('tenants.deleteFailed'));
  },
});

function deleteTenant(tenant: TenantVo): void {
  deleteMutation.mutate(tenant.id);
}

const statusMutation = useMutation({
  mutationFn: (tenant: TenantVo) =>
    http.put(`/api/system/tenants/${tenant.id}/status`, {
      status: tenant.status === 'active' ? 'suspended' : 'active',
    }),
  onSuccess: () => {
    message.success(t('tenants.statusUpdated'));
    void queryClient.invalidateQueries({ queryKey: ['tenants'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('tenants.updateFailed'));
  },
});

function toggleStatus(tenant: TenantVo): void {
  statusMutation.mutate(tenant);
}

</script>

<style scoped>
.page {
  padding: 0;
}
</style>
