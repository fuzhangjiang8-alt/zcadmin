<template>
  <!--
    运维大屏的探针与指标展示区。
    职责：渲染 4 个主动健康探针卡片与 6 个流量/连接池指标卡，统一转发下钻事件。
    边界：不发起请求、不持有轮询状态；数据由父页面通过 props 传入。
  -->
  <div class="ops-cards">
    <a-row :gutter="16" class="ops-cards-row">
      <a-col v-for="probe in probes" :key="probe.key" :xs="12" :md="12" :lg="6">
        <div class="probe-card" :class="`is-${probe.status}`" @click="emit('drill', probe.key)">
          <div class="probe-header">
            <span class="probe-name">{{ probe.name }}</span>
            <span class="status-dot" :class="probe.status"></span>
          </div>
          <div class="probe-latency">{{ probe.latencyMs }}<span class="unit">ms</span></div>
          <div class="probe-summary">{{ probe.summary }}</div>
          <div class="probe-hint">{{ statusLabel(probe.status) }} · {{ t('ops.clickForDetail') }}</div>
        </div>
      </a-col>
    </a-row>

    <a-row :gutter="16" class="ops-cards-row">
      <a-col v-for="card in cards" :key="card.key" :xs="12" :lg="4">
        <div class="metric-card clickable" @click="emit('drill', card.section)">
          <div class="metric-label">{{ card.label }}</div>
          <div class="metric-value" :class="card.tone">{{ card.value }}</div>
          <a-progress
            v-if="card.progress !== undefined"
            :percent="card.progress"
            size="small"
            :status="card.progress > 85 ? 'exception' : 'active'"
            style="margin-top: 6px"
          />
        </div>
      </a-col>
    </a-row>
  </div>
</template>

<script setup lang="ts">
/*
 * 运维大屏卡片区组件
 * 职责：纯展示探针/指标卡并把 section 下钻事件交给父页面。
 * 边界：不请求数据；i18n 只用于状态文案，卡片 label/value 由父页面组装。
 */
import { useI18n } from 'vue-i18n';
import type { ProbeItemVo, OpsMetricCard } from './types';

defineProps<{
  probes: ProbeItemVo[];
  cards: OpsMetricCard[];
}>();

const emit = defineEmits<{ drill: [section: string] }>();
const { t } = useI18n();

function statusLabel(status: string): string {
  if (status === 'healthy') return t('ops.healthy');
  if (status === 'degraded') return t('ops.degraded');
  return t('ops.unhealthy');
}
</script>

<style scoped>
/* 卡片区两行之间留白，与页面下方可交互表格保持一致节奏 */
.ops-cards-row { margin-bottom: 16px; }

/* ---- 探针卡片 ---- */
.probe-card {
  position: relative;
  padding: 18px;
  border-radius: 12px;
  cursor: pointer;
  color: #e2e8f0;
  background: linear-gradient(160deg, #1e293b, #0f172a);
  border: 1px solid rgb(148 163 184 / 18%);
  box-shadow: 0 6px 18px rgb(15 23 42 / 18%);
  transition: transform 0.2s ease, box-shadow 0.2s ease;
  overflow: hidden;
}
.probe-card::after {
  content: '';
  position: absolute;
  inset: 0 auto 0 0;
  width: 3px;
  background: var(--tone, #22c55e);
}
.probe-card.is-healthy { --tone: #22c55e; }
.probe-card.is-degraded { --tone: #f59e0b; }
.probe-card.is-unhealthy { --tone: #ef4444; }
.probe-card:hover { transform: translateY(-3px); box-shadow: 0 12px 28px rgb(15 23 42 / 32%); }
.probe-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 10px; }
.probe-name { font-weight: 600; color: #cbd5e1; }
.status-dot { width: 10px; height: 10px; border-radius: 50%; background: var(--tone); box-shadow: 0 0 8px var(--tone); }
.probe-latency { font-size: 26px; font-weight: 700; color: #f8fafc; }
.probe-latency .unit { font-size: 13px; color: #94a3b8; margin-left: 4px; }
.probe-summary { font-size: 12px; color: #94a3b8; margin-top: 4px; }
.probe-hint { font-size: 12px; color: #818cf8; margin-top: 10px; }

/* ---- 指标卡 ---- */
.metric-card {
  padding: 16px 18px;
  border-radius: 12px;
  background: #fff;
  border: 1px solid #e2e8f0;
  box-shadow: var(--ad-shadow-card);
  transition: transform 0.2s ease, box-shadow 0.2s ease;
}
.metric-card:hover { transform: translateY(-2px); box-shadow: var(--ad-shadow-hover); }
.metric-label { font-size: 13px; color: #64748b; }
.metric-value { font-size: 24px; font-weight: 700; color: #0f172a; margin-top: 4px; }
.metric-value.danger { color: #dc2626; }
</style>
