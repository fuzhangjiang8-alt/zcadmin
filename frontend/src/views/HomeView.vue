<template>
  <div class="home">
    <div class="home-hero">
      <div>
        <h2 class="hero-title">{{ t('home.hello', { name: username || '-' }) }}</h2>
        <p class="hero-sub">{{ t('home.heroSub') }}</p>
      </div>
      <a-space>
        <a-tag :color="backendOk ? 'green' : 'red'">{{ backendOk ? t('home.ok') : t('home.error') }}</a-tag>
        <a-button v-if="riskVisible" type="primary" @click="$router.push('/system/ops')">
          {{ t('home.openOpsScreen') }}
        </a-button>
      </a-space>
    </div>

    <a-row :gutter="16" style="margin-bottom: 16px">
      <a-col v-for="card in statCards" :key="card.key" :xs="12" :lg="6">
        <div class="stat-card" :style="{ '--accent': card.accent }">
          <div class="stat-icon">{{ card.icon }}</div>
          <div>
            <div class="stat-label">{{ card.label }}</div>
            <div class="stat-value">{{ card.value }}</div>
          </div>
        </div>
      </a-col>
    </a-row>

    <a-card :title="t('home.selfCheck')" :bordered="false">
      <a-descriptions :column="2" size="small">
        <a-descriptions-item :label="t('home.backend')">
          <a-tag :color="backendOk ? 'green' : 'red'">{{ backendOk ? t('home.ok') : t('home.error') }}</a-tag>
        </a-descriptions-item>
        <a-descriptions-item :label="t('home.plugin')">
          <a-tag :color="pluginOk ? 'green' : 'orange'">{{ pluginOk ? t('home.mounted') : t('home.noResponse') }}</a-tag>
        </a-descriptions-item>
        <a-descriptions-item :label="t('home.currentUser')">{{ username || '-' }}</a-descriptions-item>
        <a-descriptions-item :label="t('home.permCount')">{{ permissions.length }}</a-descriptions-item>
        <a-descriptions-item :label="t('home.activePlugins')" :span="2">
          <a-space v-if="enabledPlugins.length > 0">
            <a-tag v-for="p in enabledPlugins" :key="p.id" color="blue">{{ p.name }} ({{ p.id }})</a-tag>
          </a-space>
          <span v-else>-</span>
        </a-descriptions-item>
      </a-descriptions>
    </a-card>

    <a-card
      v-if="riskVisible"
      :title="t('home.riskTitle', { days: riskDays })"
      :bordered="false"
      style="margin-top: 16px"
    >
      <a-radio-group v-model:value="riskDays" button-style="solid" @change="loadRisk">
        <a-radio-button :value="7">{{ t('home.days', { n: 7 }) }}</a-radio-button>
        <a-radio-button :value="30">{{ t('home.days', { n: 30 }) }}</a-radio-button>
        <a-radio-button :value="90">{{ t('home.days', { n: 90 }) }}</a-radio-button>
      </a-radio-group>
      <a-divider />
      <a-row :gutter="16">
        <a-col :span="6"><a-statistic :title="t('home.successLogin')" :value="risk.success" /></a-col>
        <a-col :span="6"><a-statistic :title="t('home.failedLogin')" :value="risk.failed" /></a-col>
        <a-col :span="6"><a-statistic :title="t('home.locked')" :value="risk.locked" /></a-col>
        <a-col :span="6"><a-statistic :title="t('home.newDevices')" :value="risk.newDevices" /></a-col>
      </a-row>

      <a-divider />

      <a-row :gutter="24">
        <a-col :span="14">
          <div class="section-title">{{ t('home.dailyTrend') }}</div>
          <a-table
            :data-source="risk.byDay"
            :pagination="false"
            size="small"
            row-key="day"
            :columns="dayColumns"
          />
        </a-col>
        <a-col :span="10">
          <div class="section-title">{{ t('home.highRisk') }}</div>
          <a-empty v-if="risk.topFailed.length === 0" :description="t('home.noFailed')" />
          <a-tag v-for="u in risk.topFailed" :key="u.username" color="red" class="risk-tag">
            {{ u.username }} · {{ u.count }}
          </a-tag>
        </a-col>
      </a-row>
    </a-card>
  </div>
</template>

<script setup lang="ts">
// =============================================================================
// 工作台
//
// 系统自检 + 登录风控看板。看板仅对有审计权限的用户展示，
// 避免普通用户看到租户级安全数据。
// =============================================================================

import { computed, ref } from 'vue';
import { useQuery } from '@tanstack/vue-query';
import type { TableColumnsType } from 'ant-design-vue';
import { http } from '../api/client';
import { useI18n } from 'vue-i18n';


interface MeResponse {
  userId: string;
  username: string;
  permissions: string[];
}

interface LoginDayStat {
  day: string;
  success: number;
  failed: number;
}

interface AlertUserStat {
  username: string;
  count: number;
}

interface LoginRisk {
  days: number;
  success: number;
  failed: number;
  locked: number;
  newDevices: number;
  byDay: LoginDayStat[];
  topFailed: AlertUserStat[];
}

interface PluginInfoVo {
  id: string;
  name: string;
  version: string;
  order: number;
}

const { t } = useI18n();
const riskDays = ref(30);

const meQuery = useQuery({
  queryKey: ['me'],
  queryFn: () => http.get<MeResponse>('/api/auth/me'),
});
const pingQuery = useQuery({
  queryKey: ['demo-ping'],
  queryFn: () => http.get<string>('/api/demo/ping'),
  retry: 0,
});
const pluginsQuery = useQuery({
  queryKey: ['system-plugins'],
  queryFn: () => http.get<PluginInfoVo[]>('/api/system/plugins'),
});

const username = computed(() => meQuery.data.value?.username ?? '');
const permissions = computed(() => meQuery.data.value?.permissions ?? []);
const enabledPlugins = computed(() => pluginsQuery.data.value ?? []);
const backendOk = computed(() => meQuery.isSuccess.value);
const pluginOk = computed(() => pingQuery.isSuccess.value);

const statCards = computed(() => [
  { key: 'plugins', icon: '🧩', label: t('home.activePlugins'), value: enabledPlugins.value.length, accent: '#6366f1' },
  { key: 'perms', icon: '🛡️', label: t('home.permCount'), value: permissions.value.length, accent: '#0ea5e9' },
  { key: 'success', icon: '✅', label: t('home.successLogin'), value: risk.value.success, accent: '#22c55e' },
  { key: 'failed', icon: '⚠️', label: t('home.failedLogin'), value: risk.value.failed, accent: '#f59e0b' },
]);
const riskVisible = computed(
  () => permissions.value.includes('*') || permissions.value.includes('system:audit:list'),
);

const riskQuery = useQuery({
  queryKey: computed(() => ['login-risk', riskDays.value] as const),
  queryFn: () => http.get<LoginRisk>(`/api/system/login-risk?days=${riskDays.value}`),
  enabled: riskVisible,
});
const risk = computed<LoginRisk>(
  () =>
    riskQuery.data.value ?? {
      days: riskDays.value,
      success: 0,
      failed: 0,
      locked: 0,
      newDevices: 0,
      byDay: [],
      topFailed: [],
    },
);

const dayColumns = computed<TableColumnsType>(() => [
  { title: t('home.day'), dataIndex: 'day', key: 'day' },
  { title: t('home.success'), dataIndex: 'success', key: 'success' },
  { title: t('home.failed'), dataIndex: 'failed', key: 'failed' },
]);

function loadRisk(): void {
  void riskQuery.refetch();
}</script>

<style scoped>
.home {
  max-width: 1280px;
}

/* ---- 欢迎横幅 ---- */
.home-hero {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
  padding: 22px 26px;
  margin-bottom: 16px;
  border-radius: 14px;
  color: #f8fafc;
  background:
    radial-gradient(420px 200px at 90% 10%, rgb(99 102 241 / 40%), transparent 60%),
    linear-gradient(120deg, #312e81 0%, #4338ca 55%, #0c4a6e 100%);
  box-shadow: 0 12px 32px rgb(49 46 129 / 28%);
}
.hero-title { margin: 0; font-size: 22px; font-weight: 700; color: #fff; }
.hero-sub { margin: 4px 0 0; font-size: 13px; color: #c7d2fe; }

/* ---- 指标卡 ---- */
.stat-card {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 16px 18px;
  border-radius: 12px;
  background: #fff;
  border: 1px solid #e2e8f0;
  box-shadow: var(--ad-shadow-card);
  transition: transform 0.2s ease, box-shadow 0.2s ease;
}
.stat-card:hover { transform: translateY(-2px); box-shadow: var(--ad-shadow-hover); }
.stat-icon {
  width: 42px;
  height: 42px;
  flex: none;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 20px;
  border-radius: 10px;
  background: color-mix(in srgb, var(--accent) 12%, #fff);
}
.stat-label { font-size: 12px; color: #64748b; }
.stat-value { font-size: 22px; font-weight: 700; color: #0f172a; }

.section-title {
  margin-bottom: 8px;
  font-weight: 600;
}
.risk-tag {
  margin: 0 8px 8px 0;
}
</style>
