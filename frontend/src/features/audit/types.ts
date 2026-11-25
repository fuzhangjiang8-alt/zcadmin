// =============================================================================
// 审计配置域共享类型
// =============================================================================

/** 配置差异（导入预览 / 快照对比 / 回滚预览共用）。 */
export interface AuditConfigDiff {
  rulesAdded: string[];
  rulesRemoved: string[];
  rulesChanged: string[];
  subscriptionsAdded: string[];
  subscriptionsRemoved: string[];
  webhookChanged: boolean;
  subscriptionWebhookChanged: boolean;
}

/** 配置快照行。 */
export interface AuditSnapshotRow {
  id: string;
  createdAt: string;
  createdByName: string | null;
  name: string | null;
  note: string | null;
  bundle: unknown;
  sections: string | null;
  diffSummary: { rules: number; subscriptions: number } | null;
  diffDetail: unknown;
}


/** 审计日志行。 */
export interface AuditLogVo {
  id: string;
  userId: string | null;
  action: string;
  resourceType: string;
  resourceId: string | null;
  result: string;
  createdAt: string;
  detail: Record<string, unknown> | null;
  ip: string | null;
  userAgent: string | null;
}

/** 审计告警。 */
export interface AuditAlert {
  id: string;
  action: string;
  count: number;
  threshold: number;
  windowStart: string;
  notifyFailedAt: string | null;
  username: string | null;
}

/** 告警统计。 */
export interface AlertStats {
  total: number;
  unacked: number;
  acknowledged: number;
  avgAckMinutes: number;
  notifyFailed: number;
}

/** 投递统计。 */
export interface DeliveryStats {
  total: number;
  pending: number;
  sent: number;
  dead: number;
  avgAttempts: number;
  byUser: { username: string; total: number; sent: number; dead: number }[];
  byError: { error: string; count: number }[];
}

/** 审计统计。 */
export interface AuditStats {
  days: number;
  total: number;
  byAction: { action: string; count: number }[];
  byDay: { day: string; count: number }[];
}

/** 分页结果。 */
export interface PageResult<T> {
  data: T[];
  page: { page: number; size: number; total: number; hasNext: boolean };
}
