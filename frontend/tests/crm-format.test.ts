// =============================================================================
// CRM 格式化函数测试
// =============================================================================

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { formatAmount, stageLabel } from '../src/plugins/crm/format.ts';

test('金额按分转人民币，保留两位小数', () => {
  assert.equal(formatAmount(0), '¥0.00');
  assert.equal(formatAmount(123456), '¥1,234.56');
});

test('阶段中文名覆盖全部枚举并保留未知值', () => {
  assert.equal(stageLabel('initial'), '初步接触');
  assert.equal(stageLabel('won'), '赢单');
  assert.equal(stageLabel('unknown_stage'), 'unknown_stage');
});
