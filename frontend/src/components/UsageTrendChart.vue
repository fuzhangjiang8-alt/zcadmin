<template>
  <div class="trend-chart">
    <div class="trend-head">
      <span class="trend-title">{{ title }}</span>
      <span class="trend-value">{{ latestText }}</span>
    </div>
    <svg class="trend-svg" viewBox="0 0 600 150" preserveAspectRatio="none" role="img" :aria-label="title">
      <line v-for="i in 4" :key="i" x1="0" :y1="i * 30" x2="600" :y2="i * 30" class="grid" />
      <polyline v-if="points.length > 1" :points="polyline" class="line" />
      <circle
        v-for="(point, index) in points"
        :key="point.date"
        :cx="x(index)"
        :cy="y(point.value)"
        r="3"
        class="dot"
      />
    </svg>
    <div class="trend-axis">
      <span>{{ points[0]?.date ?? '-' }}</span>
      <span>{{ points[points.length - 1]?.date ?? '-' }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

interface TrendPoint {
  date: string;
  value: number;
}

const props = defineProps<{
  title: string;
  unit?: string;
  points: TrendPoint[];
}>();

const max = computed(() => {
  const values = props.points.map((p) => p.value);
  return values.length ? Math.max(...values) : 1;
});
const min = computed(() => {
  const values = props.points.map((p) => p.value);
  return values.length ? Math.min(...values) : 0;
});
const range = computed(() => Math.max(1, max.value - min.value));

function x(index: number): number {
  if (props.points.length <= 1) return 300;
  return (index / (props.points.length - 1)) * 600;
}

function y(value: number): number {
  return 135 - ((value - min.value) / range.value) * 110;
}

const polyline = computed(() =>
  props.points.map((point, index) => `${x(index)},${y(point.value)}`).join(' '),
);

const latestText = computed(() => {
  const last = props.points[props.points.length - 1];
  return last ? `${last.value} ${props.unit ?? ''}`.trim() : '-';
});
</script>

<style scoped>
.trend-chart {
  border: 1px solid #f0f0f0;
  border-radius: 8px;
  padding: 12px 16px 8px;
  background: #fff;
}
.trend-head {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  margin-bottom: 4px;
}
.trend-title {
  color: #1f2937;
  font-weight: 600;
}
.trend-value {
  color: #1677ff;
  font-size: 18px;
  font-weight: 600;
}
.trend-svg {
  width: 100%;
  height: 150px;
  display: block;
}
.grid {
  stroke: #f0f0f0;
  stroke-width: 1;
}
.line {
  stroke: #1677ff;
  stroke-width: 2;
  fill: none;
}
.dot {
  fill: #1677ff;
}
.trend-axis {
  display: flex;
  justify-content: space-between;
  color: #9ca3af;
  font-size: 12px;
}
</style>
