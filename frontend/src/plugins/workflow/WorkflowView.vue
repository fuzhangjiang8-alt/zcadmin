<template>
  <!--
    审批中心页（workflow 可插拔插件）
    职责：状态汇总统计卡片、审批单列表（支持状态/类型/关键字筛选）、发起申请、填写审批意见流转。
    边界：服务端状态统一走 @tanstack/vue-query；文案走 vue-i18n；保持组件 ≤ 300 行。
  -->
  <div class="page">
    <a-row :gutter="16" style="margin-bottom: 16px">
      <a-col v-for="card in statCards" :key="card.key" :span="6">
        <a-card :bordered="false" size="small">
          <a-statistic :title="card.title" :value="card.value" />
        </a-card>
      </a-col>
    </a-row>

    <a-card :bordered="false">
      <template #title>
        <div class="card-title">
          <span>{{ t('workflow.title') }}</span>
          <a-tag color="blue">{{ t('workflow.totalCount', { n: total }) }}</a-tag>
        </div>
      </template>

      <template #extra>
        <a-space>
          <a-select v-model:value="statusFilter" style="width: 120px" allow-clear :placeholder="t('workflow.allStatus')" @change="onFilterChange">
            <a-select-option value="pending">{{ t('workflow.statusPending') }}</a-select-option>
            <a-select-option value="approved">{{ t('workflow.statusApproved') }}</a-select-option>
            <a-select-option value="rejected">{{ t('workflow.statusRejected') }}</a-select-option>
            <a-select-option value="cancelled">{{ t('workflow.statusCancelled') }}</a-select-option>
          </a-select>
          <a-select v-model:value="bizTypeFilter" style="width: 130px" allow-clear :placeholder="t('workflow.allBizTypes')" @change="onFilterChange">
            <a-select-option value="contract">{{ t('workflow.typeContract') }}</a-select-option>
            <a-select-option value="discount">{{ t('workflow.typeDiscount') }}</a-select-option>
            <a-select-option value="refund">{{ t('workflow.typeRefund') }}</a-select-option>
            <a-select-option value="general">{{ t('workflow.typeGeneral') }}</a-select-option>
          </a-select>
          <a-input-search
            v-model:value="keyword"
            :placeholder="t('workflow.searchPlaceholder')"
            allow-clear
            style="width: 210px"
            @search="onFilterChange"
          />
          <a-button v-perm="PERMS.REQUEST_ADD" type="primary" @click="createOpen = true">
            {{ t('workflow.createBtn') }}
          </a-button>
        </a-space>
      </template>

      <a-table
        :columns="columns"
        :data-source="rows"
        :loading="isFetching"
        row-key="id"
        :pagination="{ current: page, pageSize: size, total, showSizeChanger: true }"
        @change="onTableChange"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'bizType'">
            <a-tag>{{ bizTypeLabel(record.bizType) }}</a-tag>
          </template>
          <template v-else-if="column.key === 'amountCents'">
            <span>{{ (record.amountCents / 100).toFixed(2) }}</span>
          </template>
          <template v-else-if="column.key === 'status'">
            <a-tag :color="statusColor(record.status)">{{ statusLabel(record.status) }}</a-tag>
          </template>
          <template v-else-if="column.key === 'decisionNote'">
            <span>{{ record.decisionNote || '—' }}</span>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-space v-if="record.status === 'pending'">
              <a-button v-perm="PERMS.REQUEST_APPROVE" type="link" size="small" @click="openDecide(record.id, 'approve')">
                {{ t('workflow.approveBtn') }}
              </a-button>
              <a-button v-perm="PERMS.REQUEST_APPROVE" type="link" danger size="small" @click="openDecide(record.id, 'reject')">
                {{ t('workflow.rejectBtn') }}
              </a-button>
              <a-button v-perm="PERMS.REQUEST_APPROVE" type="link" size="small" @click="openDecide(record.id, 'cancel')">
                {{ t('workflow.cancelBtn') }}
              </a-button>
            </a-space>
            <span v-else class="muted">—</span>
          </template>
        </template>
      </a-table>
    </a-card>

    <a-modal v-model:open="createOpen" :title="t('workflow.modalCreateTitle')" :confirm-loading="creating" @ok="submitCreate">
      <a-form layout="vertical">
        <a-form-item :label="t('workflow.fieldCode')" required><a-input v-model:value="form.code" /></a-form-item>
        <a-form-item :label="t('workflow.fieldTitle')" required><a-input v-model:value="form.title" /></a-form-item>
        <a-form-item :label="t('workflow.fieldBizType')">
          <a-select v-model:value="form.bizType">
            <a-select-option value="contract">{{ t('workflow.typeContract') }}</a-select-option>
            <a-select-option value="discount">{{ t('workflow.typeDiscount') }}</a-select-option>
            <a-select-option value="refund">{{ t('workflow.typeRefund') }}</a-select-option>
            <a-select-option value="general">{{ t('workflow.typeGeneral') }}</a-select-option>
          </a-select>
        </a-form-item>
        <a-form-item :label="t('workflow.fieldBizId')"><a-input v-model:value="form.bizId" /></a-form-item>
        <a-form-item :label="t('workflow.fieldAmountYuan')">
          <a-input-number v-model:value="form.amountYuan" :min="0" :precision="2" style="width: 100%" />
        </a-form-item>
        <a-form-item :label="t('workflow.fieldRemark')"><a-input v-model:value="form.remark" /></a-form-item>
      </a-form>
    </a-modal>

    <a-modal v-model:open="decideOpen" :title="t('workflow.modalDecideTitle')" :confirm-loading="deciding" @ok="submitDecide">
      <a-form layout="vertical">
        <a-form-item :label="t('workflow.fieldDecisionNote')"><a-input v-model:value="decideForm.note" /></a-form-item>
      </a-form>
    </a-modal>
  </div>
</template>

<script setup lang="ts">
/*
 * 审批中心页面组件（插件 workflow）
 * 职责：承载企业级审批单统计、多维筛选、发起申请及带审批意见的状态机决议操作。
 */
import { computed, reactive, ref } from 'vue';
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import type { TablePaginationConfig } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { http } from '../../api/client';
import { PERMS } from '../../permissions';

interface WorkflowRequestVo {
  id: string;
  code: string;
  title: string;
  bizType: string;
  bizId: string | null;
  status: string;
  amountCents: number;
  remark: string | null;
  decisionNote: string | null;
  createdAt: string;
}

interface WorkflowStatsVo {
  total: number;
  pending: number;
  approved: number;
  rejected: number;
  cancelled: number;
}

interface PageResult<T> {
  data: T[];
  page: { page: number; size: number; total: number; hasNext: boolean };
}

const { t } = useI18n();
const queryClient = useQueryClient();

const page = ref(1);
const size = ref(20);
const statusFilter = ref<string | undefined>(undefined);
const bizTypeFilter = ref<string | undefined>(undefined);
const keyword = ref('');
const createOpen = ref(false);
const decideOpen = ref(false);

const form = reactive({ code: '', title: '', bizType: 'contract', bizId: '', amountYuan: 0, remark: '' });
const decideForm = reactive({ id: '', action: 'approve', note: '' });

const statsQuery = useQuery({
  queryKey: ['workflow-stats'],
  queryFn: () => http.get<WorkflowStatsVo>('/api/workflow/requests/stats'),
});
const stats = computed<WorkflowStatsVo>(
  () => statsQuery.data.value ?? { total: 0, pending: 0, approved: 0, rejected: 0, cancelled: 0 },
);
const statCards = computed(() => [
  { key: 'pending', title: t('workflow.statusPending'), value: stats.value.pending },
  { key: 'approved', title: t('workflow.statusApproved'), value: stats.value.approved },
  { key: 'rejected', title: t('workflow.statusRejected'), value: stats.value.rejected },
  { key: 'cancelled', title: t('workflow.statusCancelled'), value: stats.value.cancelled },
]);

const { data, isFetching } = useQuery({
  queryKey: computed(() => ['workflow-requests', page.value, size.value, statusFilter.value, bizTypeFilter.value, keyword.value]),
  queryFn: () => {
    const params = new URLSearchParams({ page: String(page.value), size: String(size.value) });
    if (statusFilter.value) params.set('status', statusFilter.value);
    if (bizTypeFilter.value) params.set('bizType', bizTypeFilter.value);
    if (keyword.value.trim()) params.set('keyword', keyword.value.trim());
    return http.get<PageResult<WorkflowRequestVo>>(`/api/workflow/requests?${params.toString()}`);
  },
  placeholderData: keepPreviousData,
});

const rows = computed(() => data.value?.data ?? []);
const total = computed(() => data.value?.page.total ?? 0);

const columns = computed(() => [
  { title: t('workflow.colCode'), dataIndex: 'code', key: 'code', width: 140 },
  { title: t('workflow.colTitle'), dataIndex: 'title', key: 'title' },
  { title: t('workflow.colBizType'), dataIndex: 'bizType', key: 'bizType', width: 115 },
  { title: t('workflow.colAmount'), dataIndex: 'amountCents', key: 'amountCents', width: 115 },
  { title: t('workflow.colStatus'), dataIndex: 'status', key: 'status', width: 100 },
  { title: t('workflow.colDecisionNote'), dataIndex: 'decisionNote', key: 'decisionNote', width: 160 },
  { title: t('workflow.colCreatedAt'), dataIndex: 'createdAt', key: 'createdAt', width: 170 },
  { title: t('workflow.colActions'), key: 'actions', width: 170 },
]);

function statusColor(status: string): string {
  if (status === 'approved') return 'green';
  if (status === 'rejected') return 'red';
  if (status === 'cancelled') return 'default';
  return 'processing';
}

function statusLabel(status: string): string {
  if (status === 'approved') return t('workflow.statusApproved');
  if (status === 'rejected') return t('workflow.statusRejected');
  if (status === 'cancelled') return t('workflow.statusCancelled');
  return t('workflow.statusPending');
}

function bizTypeLabel(bizType: string): string {
  if (bizType === 'contract') return t('workflow.typeContract');
  if (bizType === 'discount') return t('workflow.typeDiscount');
  if (bizType === 'refund') return t('workflow.typeRefund');
  return t('workflow.typeGeneral');
}

function onFilterChange() {
  page.value = 1;
}

function onTableChange(pagination: TablePaginationConfig) {
  page.value = pagination.current ?? 1;
  size.value = pagination.pageSize ?? 20;
}

function refreshAll() {
  queryClient.invalidateQueries({ queryKey: ['workflow-requests'] });
  queryClient.invalidateQueries({ queryKey: ['workflow-stats'] });
}

const { mutate: createMutate, isPending: creating } = useMutation({
  mutationFn: () =>
    http.post<WorkflowRequestVo>('/api/workflow/requests', {
      code: form.code.trim(),
      title: form.title.trim(),
      bizType: form.bizType,
      bizId: form.bizId.trim() || null,
      amountCents: Math.round((form.amountYuan || 0) * 100),
      remark: form.remark.trim() || null,
    }),
  onSuccess: () => {
    message.success(t('workflow.createdSuccess'));
    createOpen.value = false;
    Object.assign(form, { code: '', title: '', bizId: '', amountYuan: 0, remark: '' });
    refreshAll();
  },
  onError: (err: Error) => message.error(err.message),
});

function submitCreate() {
  if (!form.code.trim() || !form.title.trim()) {
    message.warning(t('workflow.requiredFields'));
    return;
  }
  createMutate();
}

function openDecide(id: string, action: string) {
  decideForm.id = id;
  decideForm.action = action;
  decideForm.note = '';
  decideOpen.value = true;
}

const { mutate: decideMutate, isPending: deciding } = useMutation({
  mutationFn: () =>
    http.post<WorkflowRequestVo>(`/api/workflow/requests/${decideForm.id}/decide`, {
      action: decideForm.action,
      note: decideForm.note.trim() || null,
    }),
  onSuccess: () => {
    message.success(t('workflow.decidedSuccess'));
    decideOpen.value = false;
    refreshAll();
  },
  onError: (err: Error) => message.error(err.message),
});

function submitDecide() {
  decideMutate();
}
</script>
