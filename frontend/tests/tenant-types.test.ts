// =============================================================================
// 租户域纯函数测试
// =============================================================================

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { storageText } from '../src/features/tenants/types.ts';

const base = {
  storageUsed: 5 * 1024 * 1024,
  maxStorageBytes: 0,
};

test('无上限时只显示已用', () => {
  assert.equal(storageText(base as never), '5.0 MB');
});

test('有上限时显示 已用/上限', () => {
  const tenant = { ...base, maxStorageBytes: 10 * 1024 * 1024 };
  assert.equal(storageText(tenant as never), '5.0/10 MB');
});

test('空值返回占位符', () => {
  assert.equal(storageText(undefined), '-');
});
