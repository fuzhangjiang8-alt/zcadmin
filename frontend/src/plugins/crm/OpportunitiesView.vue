<template>
  <div class="page">
    <a-card :bordered="false" :title="t('crm.opportunitiesTitle')">
      <template #extra>
        <a-space>
          <a-input-search
            v-model:value="searchText"
            :placeholder="t('crm.searchOpportunity')"
            allow-clear
            style="width: 240px"
            @search="onSearch"
          />
          <a-button v-perm="PERMS.OPPORTUNITY_LIST" :loading="exporting" @click="exportNow">{{ t('crm.exportBtn') }}</a-button>
          <a-upload :before-upload="onImport" :show-upload-list="false" accept=".xlsx">
            <a-button v-perm="PERMS.OPPORTUNITY_ADD" :loading="importing">{{ t('crm.importBtn') }}</a-button>
          </a-upload>
          <a-button v-perm="PERMS.OPPORTUNITY_ADD" type="primary" @click="openCreate">
            {{ t('crm.newOpportunity') }}
          </a-button>
        </a-space>
      </template>

      <a-table
        :columns="columns"
        :data-source="rows"
        :loading="loading"
        row-key="id"
        :pagination="{
          current: page,
          pageSize: size,
          total,
          showSizeChanger: true,
          showTotal: (total: number) => t('crm.totalOpportunities', { n: total }),
        }"
        @change="onTableChange"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'name'">
            <router-link :to="{ name: 'crm-opportunity-detail', params: { id: record.id } }">
              {{ record.name }}
            </router-link>
          </template>
          <template v-else-if="column.key === 'customer'">{{ customerName(record.customerId) }}</template>
          <template v-else-if="column.key === 'amount'">{{ formatAmount(record.amountCents) }}</template>
          <template v-else-if="column.key === 'stage'">
            <a-tag :color="stageColor(record.stage)">{{ stageText(record.stage) }}</a-tag>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-space>
              <a-button v-perm="PERMS.OPPORTUNITY_EDIT" type="link" size="small" @click="openEdit(record)">
                {{ t('crm.edit') }}
              </a-button>
              <a-popconfirm :title="t('crm.deleteConfirmOpportunity')" @confirm="onDelete(record)">
                <a-button v-perm="PERMS.OPPORTUNITY_DELETE" type="link" size="small" danger>
                  {{ t('crm.delete') }}
                </a-button>
              </a-popconfirm>
            </a-space>
          </template>
        </template>
      </a-table>
    </a-card>

    <OpportunityEditModal v-model:open="editOpen" :opportunity="editing" @saved="onSaved" />
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../../permissions';
// =============================================================================
// CRM 商机
//
// 金额用「分」存储（amountCents），前端按元展示，避免浮点误差。
// 商机必须挂在客户下；编辑时不允许更换客户。
// =============================================================================

import { computed, onMounted, ref, watch } from 'vue';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { useI18n } from 'vue-i18n';
import { useRouter } from 'vue-router';
import { Modal, message } from 'ant-design-vue';
import type { TablePaginationConfig } from 'ant-design-vue';
import { ApiError, http } from '../../api/client';
import { useImport } from './useImport';
import OpportunityEditModal from './OpportunityEditModal.vue';
import { useExport } from './useExport';

interface OpportunityVo {
  id: string;
  customerId: string;
  name: string;
  amountCents: number | null;
  stage: string;
}

interface CustomerVo {
  id: string;
  name: string;
}

interface PageResult<T> {
  data: T[];
  page: { page: number; size: number; total: number; hasNext: boolean };
}

const stageOptions = computed(() => [
  { label: t('crm.stageInitial'), value: 'initial' },
  { label: t('crm.stageProposal'), value: 'proposal' },
  { label: t('crm.stageNegotiation'), value: 'negotiation' },
  { label: t('crm.stageWon'), value: 'won' },
  { label: t('crm.stageLost'), value: 'lost' },
]);

const router = useRouter();
const { t } = useI18n();
const queryClient = useQueryClient();
const searchText = ref('');
const keyword = ref('');
const page = ref(1);
const size = ref(20);
const customers = ref<CustomerVo[]>([]);

const query = useQuery({
  queryKey: computed(() => ['crm-opportunities', page.value, size.value, keyword.value] as const),
  queryFn: () => {
    const params = new URLSearchParams({ page: String(page.value), size: String(size.value) });
    if (keyword.value.trim()) params.set('keyword', keyword.value.trim());
    return http.get<PageResult<OpportunityVo>>(`/api/crm/opportunities?${params.toString()}`);
  },
  placeholderData: keepPreviousData,
});
const rows = computed(() => query.data.value?.data ?? []);
const total = computed(() => query.data.value?.page.total ?? 0);
const loading = computed(() => query.isFetching.value);

watch(query.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('crm.loadFailed'));
});

const { exporting, exportNow } = useExport('crm.opportunities');
const { importing, importFile } = useImport('crm.opportunities');

function onSearch(): void {
  page.value = 1;
  keyword.value = searchText.value;
  void queryClient.invalidateQueries({ queryKey: ['crm-opportunities'] });
}

function onImport(file: File): boolean {
  void importFile(file).then((ok) => {
    if (ok) void queryClient.invalidateQueries({ queryKey: ['crm-opportunities'] });
  });
  return false;
}

const columns = computed(() => [
  { title: t('crm.colOppName'), dataIndex: 'name', key: 'name', width: 220 },
  { title: t('crm.colCustomer'), dataIndex: 'customerId', key: 'customer', width: 180 },
  { title: t('crm.colAmount'), dataIndex: 'amountCents', key: 'amount', width: 140 },
  { title: t('crm.colStage'), dataIndex: 'stage', key: 'stage', width: 120 },
  { title: t('crm.colActions'), key: 'actions', width: 150 },
]);

const stageText = (stage: string): string =>
  stageOptions.value.find((s: { value: string; label: string }) => s.value === stage)?.label ?? stage;
const stageColor = (stage: string): string =>
  ({ initial: 'default', proposal: 'blue', negotiation: 'orange', won: 'green', lost: 'red' })[
    stage
  ] ?? 'default';

function customerName(id: string): string {
  return customers.value.find((c) => c.id === id)?.name ?? id.slice(0, 8);
}

function formatAmount(cents: number | null): string {
  if (cents == null) return '-';
  return `¥${(cents / 100).toLocaleString('zh-CN', { minimumFractionDigits: 2 })}`;
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

const editOpen = ref(false);
const editing = ref<OpportunityVo | null>(null);

function openCreate(): void {
  editing.value = null;
  editOpen.value = true;
}

function openEdit(record: OpportunityVo): void {
  editing.value = record;
  editOpen.value = true;
}

/** 赢单后引导创建合同（业务流程断点在这里最容易丢单）。 */
function onSaved(saved: OpportunityVo): void {
  void queryClient.invalidateQueries({ queryKey: ['crm-opportunities'] });
  if (saved.stage === 'won') {
    Modal.confirm({
      title: t('crm.oppWonTitle'),
      content: t('crm.oppWonContent'),
      okText: t('crm.goCreateContract'),
      cancelText: t('crm.later'),
      onOk: () => {
        void router.push({ name: 'crm-opportunity-detail', params: { id: saved.id } });
      },
    });
  }
}

const deleteMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/crm/opportunities/${id}`),
  onSuccess: () => {
    message.success(t('crm.deleted'));
    void queryClient.invalidateQueries({ queryKey: ['crm-opportunities'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('crm.deleteFailed'));
  },
});

function onDelete(record: OpportunityVo): void {
  deleteMutation.mutate(record.id);
}

onMounted(() => {
  void loadCustomers().catch(() => undefined);
});
</script>

<style scoped>
.page {
  padding: 0;
}
</style>
