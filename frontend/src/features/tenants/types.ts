// =============================================================================
// 租户域共享类型
// =============================================================================

/** 租户列表行。 */
export interface TenantVo {
  id: string;
  code: string;
  name: string;
  status: string;
  userCount: number;
  maxUsers: number;
  maxStorageBytes: number;
  storageUsed: number;
  pricePerUserMonthCents: number;
  pricePerGbMonthCents: number;
  currency: string;
  createdAt: string;
}

/** 租户详情汇总。 */
export interface TenantSummary {
  id: string;
  code: string;
  name: string;
  status: string;
  createdAt: string;
  maxUsers: number;
  maxStorageBytes: number;
  userCount: number;
  fileCount: number;
  storageUsed: number;
  auditCount: number;
  sessionCount: number;
  customerCount: number;
  opportunityCount: number;
  contactCount: number;
  contractCount: number;
  paymentCount: number;
  recentAudits: { action: string; createdAt: string }[];
}

/** 用量快照点。 */
export interface UsagePoint {
  snapshotDate: string;
  userCount: number;
  fileCount: number;
  storageUsed: number;
  customerCount: number;
  contractCount: number;
}

/** 账单行。 */
export interface TenantBillRow {
  id: string;
  tenantCode: string;
  tenantName: string;
  billMonth: string;
  currency: string;
  avgUsers: number;
  avgStorageBytes: number;
  totalCents: number;
  paidCents: number;
  status: string;
  createdAt: string;
}

/** 本月账单预估。 */
export interface TenantBill {
  month: string;
  currency: string;
  avgUsers: number;
  avgStorageBytes: number;
  userAmountCents: number;
  storageAmountCents: number;
  totalCents: number;
}

/** 催收记录。 */
export interface BillCollection {
  id: string;
  note: string;
  promisedAmountCents: number | null;
  promisedDate: string | null;
  createdAt: string;
}

/** 对账行。 */
export interface ReconciliationRow {
  tenantId: string;
  tenantCode: string;
  tenantName: string;
  billCount: number;
  totalCents: number;
  paidCents: number;
  refundedCents: number;
  netCents: number;
  outstandingCents: number;
}

/** 存储用量文案。 */
export function storageText(tenant?: TenantVo | TenantSummary): string {
  if (!tenant) return '-';
  const used = (tenant.storageUsed / 1024 / 1024).toFixed(1);
  if (tenant.maxStorageBytes <= 0) return `${used} MB`;
  const max = (tenant.maxStorageBytes / 1024 / 1024).toFixed(0);
  return `${used}/${max} MB`;
}
