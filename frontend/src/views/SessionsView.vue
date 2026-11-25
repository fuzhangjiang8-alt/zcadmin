<template>
  <div class="page">
    <a-card :bordered="false" :title="t('sessions.title')">
      <template #extra>
        <a-button :loading="loading" @click="load">{{ t('sessions.refresh') }}</a-button>
      </template>

      <a-alert
        type="info"
        show-icon
        :message="t('sessions.info')"
        style="margin-bottom: 16px"
      />

      <a-table :columns="columns" :data-source="rows" :loading="loading" row-key="id" :pagination="false">
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'createdAt'">{{ format(record.createdAt) }}</template>
          <template v-else-if="column.key === 'expiresAt'">{{ format(record.expiresAt) }}</template>
          <template v-else-if="column.key === 'actions'">
            <a-popconfirm :title="t('sessions.revokeConfirm')" @confirm="revoke(record)">
              <a-button v-perm="PERMS.SESSION_REVOKE" type="link" size="small" danger>
                {{ t('sessions.revoke') }}
              </a-button>
            </a-popconfirm>
          </template>
        </template>
      </a-table>
    </a-card>

    <a-card :bordered="false" :title="t('sessions.tokenTitle')" style="margin-top: 16px">
      <template #extra>
        <a-button type="primary" @click="openTokenModal">{{ t('sessions.tokenNew') }}</a-button>
      </template>

      <a-alert
        type="warning"
        show-icon
        :message="t('sessions.tokenInfo')"
        style="margin-bottom: 16px"
      />

      <a-table
        :columns="tokenColumns"
        :data-source="tokens"
        :loading="tokenLoading"
        row-key="id"
        :pagination="false"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'createdAt'">{{ format(record.createdAt) }}</template>
          <template v-else-if="column.key === 'expiresAt'">
            {{ record.expiresAt ? format(record.expiresAt) : t('sessions.neverExpires') }}
          </template>
          <template v-else-if="column.key === 'lastUsedAt'">
            {{ record.lastUsedAt ? format(record.lastUsedAt) : t('sessions.neverUsed') }}
          </template>
          <template v-else-if="column.key === 'scopes'">
            {{ record.scopes.length ? record.scopes.join(', ') : t('sessions.allScopes') }}
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-popconfirm :title="t('sessions.revokeTokenConfirm')" @confirm="revokeToken(record)">
              <a-button type="link" size="small" danger>{{ t('sessions.revokeToken') }}</a-button>
            </a-popconfirm>
          </template>
        </template>
      </a-table>
    </a-card>

    <a-modal
      v-model:open="tokenModalOpen"
      :title="t('sessions.tokenModalTitle')"
      :confirm-loading="tokenSaving"
      @ok="createToken"
    >
      <a-form layout="vertical">
        <a-form-item :label="t('sessions.tokenName')">
          <a-input v-model:value="tokenForm.name" :placeholder="t('sessions.tokenNamePlaceholder')" :maxlength="100" />
        </a-form-item>
        <a-form-item :label="t('sessions.tokenDays')">
          <a-input-number v-model:value="tokenForm.days" :min="1" :max="3650" style="width: 100%" />
        </a-form-item>
        <a-form-item :label="t('sessions.tokenScopes')">
          <a-input v-model:value="tokenForm.scopes" :placeholder="t('sessions.tokenScopesPlaceholder')" />
        </a-form-item>
      </a-form>
    </a-modal>

    <a-modal v-model:open="tokenResultOpen" :title="t('sessions.tokenCreatedTitle')" :footer="null" width="640px">
      <a-alert type="warning" show-icon :message="t('sessions.tokenCopyHint')" style="margin-bottom: 12px" />
      <a-textarea :value="createdToken" :rows="3" readonly />
      <a-button type="primary" style="margin-top: 12px" @click="copyToken">{{ t('sessions.copy') }}</a-button>
    </a-modal>
  </div>
</template>

<script setup lang="ts">
import { PERMS } from '../permissions';
// =============================================================================
// 在线会话 + 个人访问令牌（PAT）
// =============================================================================

import { computed, reactive, ref, watch } from 'vue';
import { useMutation, useQuery, useQueryClient } from '@tanstack/vue-query';
import { message } from 'ant-design-vue';
import { useI18n } from 'vue-i18n';
import { ApiError, http } from '../api/client';

interface SessionVo {
  id: string;
  userId: string;
  username: string;
  deptId: string | null;
  ip: string | null;
  userAgent: string | null;
  createdAt: string;
  expiresAt: string;
}

interface TokenVo {
  id: string;
  name: string;
  createdAt: string;
  expiresAt: string | null;
  lastUsedAt: string | null;
  scopes: string[];
}

const { t } = useI18n();
const queryClient = useQueryClient();

const sessionsQuery = useQuery({
  queryKey: ['sessions'],
  queryFn: () => http.get<SessionVo[]>('/api/system/sessions'),
});
const rows = computed(() => sessionsQuery.data.value ?? []);
const loading = computed(() => sessionsQuery.isFetching.value);

const tokensQuery = useQuery({
  queryKey: ['tokens'],
  queryFn: () => http.get<TokenVo[]>('/api/auth/tokens'),
});
const tokens = computed(() => tokensQuery.data.value ?? []);
const tokenLoading = computed(() => tokensQuery.isFetching.value);

watch(sessionsQuery.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('sessions.loadFailed'));
});
watch(tokensQuery.error, (error) => {
  if (error) message.error(error instanceof ApiError ? error.message : t('sessions.tokenLoadFailed'));
});

const columns = computed(() => [
  { title: t('sessions.colUser'), dataIndex: 'username', key: 'username' },
  { title: t('sessions.colIp'), dataIndex: 'ip', key: 'ip', width: 140 },
  { title: t('sessions.colDevice'), dataIndex: 'userAgent', key: 'userAgent', ellipsis: true },
  { title: t('sessions.colCreated'), dataIndex: 'createdAt', key: 'createdAt', width: 190 },
  { title: t('sessions.colExpires'), dataIndex: 'expiresAt', key: 'expiresAt', width: 190 },
  { title: t('sessions.colActions'), key: 'actions', width: 120 },
]);

const tokenModalOpen = ref(false);
const tokenSaving = ref(false);
const tokenForm = reactive<{ name: string; days: number | null; scopes: string }>({
  name: '',
  days: 90,
  scopes: '',
});
const tokenResultOpen = ref(false);
const createdToken = ref('');
const tokenColumns = computed(() => [
  { title: t('sessions.colName'), dataIndex: 'name', key: 'name' },
  { title: t('sessions.colCreated'), dataIndex: 'createdAt', key: 'createdAt', width: 190 },
  { title: t('sessions.colExpires'), dataIndex: 'expiresAt', key: 'expiresAt', width: 190 },
  { title: t('sessions.colLastUsed'), dataIndex: 'lastUsedAt', key: 'lastUsedAt', width: 190 },
  { title: t('sessions.colScopes'), dataIndex: 'scopes', key: 'scopes' },
  { title: t('sessions.colActions'), key: 'actions', width: 90 },
]);

function load(): void {
  void sessionsQuery.refetch();
}

function format(value: string): string {
  return value.replace('T', ' ').slice(0, 19);
}

const revokeMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/system/sessions/${id}`),
  onSuccess: () => {
    message.success(t('sessions.revoked'));
    void queryClient.invalidateQueries({ queryKey: ['sessions'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('sessions.actionFailed'));
  },
});

function revoke(record: SessionVo): void {
  revokeMutation.mutate(record.id);
}

function openTokenModal(): void {
  tokenForm.name = '';
  tokenForm.days = 90;
  tokenForm.scopes = '';
  tokenModalOpen.value = true;
}

const createTokenMutation = useMutation({
  mutationFn: () =>
    http.post<{ token: string }>('/api/auth/tokens', {
      name: tokenForm.name.trim(),
      expiresInDays: tokenForm.days,
      scopes: tokenForm.scopes
        ? tokenForm.scopes.split(',').map((s) => s.trim()).filter(Boolean)
        : null,
    }),
  onSuccess: (res) => {
    createdToken.value = res.token;
    tokenModalOpen.value = false;
    tokenResultOpen.value = true;
    void queryClient.invalidateQueries({ queryKey: ['tokens'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('sessions.createFailed'));
  },
  onSettled: () => {
    tokenSaving.value = false;
  },
});

function createToken(): void {
  if (!tokenForm.name.trim()) {
    message.warning(t('sessions.nameRequired'));
    return;
  }
  tokenSaving.value = true;
  createTokenMutation.mutate();
}

const revokeTokenMutation = useMutation({
  mutationFn: (id: string) => http.delete(`/api/auth/tokens/${id}`),
  onSuccess: () => {
    message.success(t('sessions.tokenRevoked'));
    void queryClient.invalidateQueries({ queryKey: ['tokens'] });
  },
  onError: (error) => {
    message.error(error instanceof ApiError ? error.message : t('sessions.revokeFailed'));
  },
});

function revokeToken(record: TokenVo): void {
  revokeTokenMutation.mutate(record.id);
}

async function copyToken(): Promise<void> {
  await navigator.clipboard.writeText(createdToken.value);
  message.success(t('sessions.copied'));
}
</script>
