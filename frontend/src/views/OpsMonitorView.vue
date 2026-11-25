<template>
  <!--
    系统运维与探针监控大屏（/system/ops）
    职责：实时展示服务器运行指标、4 大主动健康探针、HTTP 请求统计与路由性能排行；
          支持点击任意探针卡片打开深度诊断抽屉，或点击单条路由查看接口详情弹窗。
  -->
  <div class="ops-page">
    <div class="ops-hero">
      <div class="hero-left">
        <h2 class="hero-title">{{ t('ops.title') }}</h2>
        <div class="hero-meta">
          <a-tag color="blue">v{{ overview?.server.version ?? '0.1.0' }}</a-tag>
          <a-tag color="green">{{ t('ops.uptime', { time: formatUptime(overview?.server.uptimeSecs ?? 0) }) }}</a-tag>
          <span class="hero-desc">{{ overview?.server.os }} / {{ overview?.server.arch }} · PID {{ overview?.server.pid }}</span>
        </div>
      </div>
      <a-space>
        <span class="hero-switch-label">{{ t('ops.autoRefresh') }}</span>
        <a-switch v-model:checked="autoRefresh" />
        <a-button type="primary" :loading="isFetching" @click="probeNow">{{ t('ops.probeNow') }}</a-button>
      </a-space>
    </div>

    <!-- 探针卡片与指标卡拆到 features/ops，页面只保留数据组装与下钻编排 -->
    <OpsProbeCards
      :probes="overview?.probes ?? []"
      :cards="metricCards"
      @drill="openSectionDetail"
    />

    <a-card :bordered="false" :title="t('ops.routeTableTitle')">
      <a-table
        :columns="routeColumns"
        :data-source="overview?.routes ?? []"
        :loading="isFetching"
        :row-key="(r: RouteMetricVo) => `${r.method}:${r.path}`"
        size="small"
        :pagination="{ pageSize: 15 }"
      >
        <template #bodyCell="{ column, record }">
          <template v-if="column.key === 'method'">
            <a-tag :color="methodColor(record.method)">{{ record.method }}</a-tag>
          </template>
          <template v-else-if="column.key === 'err5xx'">
            <a-tag v-if="record.err5xx > 0" color="red">{{ record.err5xx }}</a-tag>
            <span v-else>0</span>
          </template>
          <template v-else-if="column.key === 'actions'">
            <a-button type="link" size="small" @click="openRouteModal(record)">{{ t('ops.viewRoute') }}</a-button>
          </template>
        </template>
      </a-table>
    </a-card>

    <a-drawer
      v-model:open="drawerOpen"
      :title="t('ops.drawerTitle', { section: activeSection })"
      width="680"
    >
      <a-spin :spinning="detailLoading">
        <a-descriptions bordered :column="1" size="small" style="margin-bottom: 16px">
          <a-descriptions-item v-for="item in detailData?.items ?? []" :key="item.label" :label="item.label">
            <a-tag :color="item.level === 'warn' ? 'red' : item.level === 'ok' ? 'green' : 'blue'">
              {{ item.value }}
            </a-tag>
          </a-descriptions-item>
        </a-descriptions>

        <a-table
          v-if="(detailData?.routes?.length ?? 0) > 0"
          :columns="routeColumns"
          :data-source="detailData?.routes ?? []"
          :row-key="(r: RouteMetricVo) => `${r.method}:${r.path}`"
          size="small"
          :pagination="{ pageSize: 10 }"
        />
      </a-spin>
    </a-drawer>

    <a-modal v-model:open="routeModalOpen" :title="t('ops.routeModalTitle')" :footer="null">
      <div v-if="selectedRoute">
        <a-descriptions bordered :column="1" size="small">
          <a-descriptions-item :label="t('ops.colMethod')">
            <a-tag :color="methodColor(selectedRoute.method)">{{ selectedRoute.method }}</a-tag>
          </a-descriptions-item>
          <a-descriptions-item :label="t('ops.colPath')">{{ selectedRoute.path }}</a-descriptions-item>
          <a-descriptions-item :label="t('ops.colTotal')">{{ selectedRoute.total }}</a-descriptions-item>
          <a-descriptions-item :label="t('ops.successRate')">
            <a-progress :percent="routeSuccessRate(selectedRoute)" size="small" />
          </a-descriptions-item>
          <a-descriptions-item label="2xx / 4xx / 5xx">
            {{ selectedRoute.ok2xx }} / {{ selectedRoute.err4xx }} / {{ selectedRoute.err5xx }}
          </a-descriptions-item>
          <a-descriptions-item :label="t('ops.avgMaxLatency')">
            {{ selectedRoute.avgMs }} ms / {{ selectedRoute.maxMs }} ms
          </a-descriptions-item>
          <a-descriptions-item :label="t('ops.colLastStatus')">{{ selectedRoute.lastStatus }}</a-descriptions-item>
          <a-descriptions-item :label="t('ops.lastSeen')">{{ selectedRoute.lastSeenAt ?? '-' }}</a-descriptions-item>
        </a-descriptions>
      </div>
    </a-modal>
  </div>
</template>

<script setup lang="ts">
/*
 * 系统运维与探针监控大屏组件
 * 职责：轮询或手动拉取 /api/system/ops/overview，支持点击探针下钻 /api/system/ops/detail/{section}。
 */
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { useQuery } from '@tanstack/vue-query';
import { useI18n } from 'vue-i18n';
import { http } from '../api/client';
import OpsProbeCards from '../features/ops/OpsProbeCards.vue';
import type { OpsDetailVo, OpsMetricCard, OpsOverviewVo, RouteMetricVo } from '../features/ops/types';

const { t } = useI18n();
const autoRefresh = ref(true);
const drawerOpen = ref(false);
const activeSection = ref('database');
const detailLoading = ref(false);
const detailData = ref<OpsDetailVo | null>(null);
const routeModalOpen = ref(false);
const selectedRoute = ref<RouteMetricVo | null>(null);

const forceRefresh = ref(false);
const { data: overview, isFetching, refetch } = useQuery({
  queryKey: ['system-ops-overview'],
  queryFn: () => {
    const url = forceRefresh.value ? '/api/system/ops/overview?refresh=true' : '/api/system/ops/overview';
    forceRefresh.value = false;
    return http.get<OpsOverviewVo>(url);
  },
});

function probeNow(): void {
  forceRefresh.value = true;
  void refetch();
}

let timer: ReturnType<typeof setInterval> | null = setInterval(() => {
  if (autoRefresh.value) void refetch();
}, 5000);

watch(autoRefresh, (enabled) => {
  if (timer) clearInterval(timer);
  timer = enabled ? setInterval(() => void refetch(), 5000) : null;
});

onBeforeUnmount(() => {
  if (timer) clearInterval(timer);
});

const poolPercent = computed(() => {
  const max = overview.value?.server.dbPoolMax ?? 20;
  const size = overview.value?.server.dbPoolSize ?? 0;
  return max > 0 ? Math.round((size / max) * 100) : 0;
});

const metricCards = computed<OpsMetricCard[]>(() => {
  const traffic = overview.value?.traffic;
  return [
    { key: 'total', label: t('ops.totalRequests'), value: traffic?.totalRequests ?? 0, section: 'requests' },
    { key: 'inFlight', label: t('ops.inFlight'), value: traffic?.inFlight ?? 0, section: 'requests' },
    { key: 'ok2xx', label: t('ops.status2xx'), value: traffic?.status2xx ?? 0, section: 'requests' },
    { key: 'err', label: t('ops.status4xx5xx'), value: `${traffic?.status4xx ?? 0} / ${traffic?.status5xx ?? 0}`, section: 'requests', tone: (traffic?.status5xx ?? 0) > 0 ? 'danger' : '' },
    { key: 'latency', label: t('ops.avgMaxLatency'), value: `${traffic?.avgLatencyMs ?? 0} / ${traffic?.maxLatencyMs ?? 0} ms`, section: 'requests' },
    { key: 'pool', label: t('ops.poolUsage'), value: `${overview.value?.server.dbPoolSize ?? 0} / ${overview.value?.server.dbPoolMax ?? 20}`, section: 'database', progress: poolPercent.value },
  ];
});

const routeColumns = computed(() => [
  { title: t('ops.colMethod'), dataIndex: 'method', key: 'method', width: 90 },
  { title: t('ops.colPath'), dataIndex: 'path', key: 'path' },
  { title: t('ops.colTotal'), dataIndex: 'total', key: 'total', width: 100 },
  { title: t('ops.col2xx'), dataIndex: 'ok2xx', key: 'ok2xx', width: 80 },
  { title: t('ops.col4xx'), dataIndex: 'err4xx', key: 'err4xx', width: 80 },
  { title: t('ops.col5xx'), dataIndex: 'err5xx', key: 'err5xx', width: 80 },
  { title: t('ops.colAvgMs'), dataIndex: 'avgMs', key: 'avgMs', width: 110 },
  { title: t('ops.colMaxMs'), dataIndex: 'maxMs', key: 'maxMs', width: 110 },
  { title: t('ops.colLastStatus'), dataIndex: 'lastStatus', key: 'lastStatus', width: 90 },
  { title: t('ops.colActions'), key: 'actions', width: 90 },
]);

function methodColor(method: string): string {
  return { GET: 'blue', POST: 'green', PUT: 'orange', PATCH: 'cyan', DELETE: 'red' }[method] ?? 'default';
}

function formatUptime(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  return `${h}h ${m}m ${s}s`;
}

function routeSuccessRate(r: RouteMetricVo): number {
  return r.total > 0 ? Math.round((r.ok2xx / r.total) * 100) : 100;
}

async function openSectionDetail(section: string): Promise<void> {
  activeSection.value = section;
  drawerOpen.value = true;
  detailLoading.value = true;
  try {
    detailData.value = await http.get<OpsDetailVo>(`/api/system/ops/detail/${section}`);
  } finally {
    detailLoading.value = false;
  }
}

function openRouteModal(record: RouteMetricVo): void {
  selectedRoute.value = record;
  routeModalOpen.value = true;
}
</script>

<style scoped>
/* ---- 顶部品牌横幅 ---- */
.ops-hero {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 16px;
  padding: 22px 24px;
  margin-bottom: 16px;
  border-radius: 14px;
  color: #e2e8f0;
  background:
    radial-gradient(420px 200px at 85% 0%, rgb(99 102 241 / 35%), transparent 60%),
    linear-gradient(135deg, #111827 0%, #1e1b4b 60%, #0c4a6e 100%);
  box-shadow: 0 12px 32px rgb(15 23 42 / 30%);
}
.hero-title { margin: 0; font-size: 20px; font-weight: 700; color: #fff; }
.hero-meta { display: flex; align-items: center; gap: 8px; margin-top: 6px; }
.hero-desc { font-size: 12px; color: #94a3b8; }
.hero-switch-label { color: #cbd5e1; font-size: 13px; }
</style>
