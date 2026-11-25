// =============================================================================
// CRM 展示格式化（纯函数，便于复用与单测）
// =============================================================================

/** 金额（分）→ 人民币展示。 */
export function formatAmount(cents: number): string {
  return `¥${(cents / 100).toLocaleString('zh-CN', { minimumFractionDigits: 2 })}`;
}

/** 商机阶段中文名。 */
export function stageLabel(stage: string): string {
  return (
    { initial: '初步接触', proposal: '方案/报价', negotiation: '谈判', won: '赢单', lost: '输单' }[
      stage
    ] ?? stage
  );
}
