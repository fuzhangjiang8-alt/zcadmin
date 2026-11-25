// =============================================================================
// 运维监控与探针大屏共享类型
// =============================================================================

export interface RouteMetricVo {
  method: string;
  path: string;
  total: number;
  ok2xx: number;
  err4xx: number;
  err5xx: number;
  avgMs: number;
  maxMs: number;
  lastStatus: number;
  lastSeenAt: string | null;
}

export interface ProbeItemVo {
  key: string;
  name: string;
  status: string;
  latencyMs: number;
  summary: string;
}

/** 指标卡展示模型：由页面从 overview 组装，组件只负责渲染。 */
export interface OpsMetricCard {
  key: string;
  label: string;
  value: string | number;
  section: string;
  tone?: string;
  progress?: number;
}

export interface OpsOverviewVo {
  server: {
    version: string;
    os: string;
    arch: string;
    cpuCores: number;
    pid: number;
    uptimeSecs: number;
    dbPoolSize: number;
    dbPoolIdle: number;
    dbPoolMax: number;
  };
  probes: ProbeItemVo[];
  traffic: {
    totalRequests: number;
    inFlight: number;
    status2xx: number;
    status4xx: number;
    status5xx: number;
    errorRatePercent: number;
    avgLatencyMs: number;
    maxLatencyMs: number;
  };
  routes: RouteMetricVo[];
}

export interface OpsDetailVo {
  section: string;
  status: string;
  items: { label: string; value: string; level: string }[];
  routes: RouteMetricVo[];
}
