// =============================================================================
// 角色域纯函数测试
// =============================================================================

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { scopeText } from '../src/features/roles/types.ts';

test('数据范围文案覆盖全部枚举', () => {
  assert.equal(scopeText('all'), '全部数据');
  assert.equal(scopeText('dept_and_child'), '本部门及下级');
  assert.equal(scopeText('dept'), '本部门');
  assert.equal(scopeText('custom'), '自定义部门');
  assert.equal(scopeText('self'), '仅本人');
});

test('未知范围原样返回，便于发现后端新增枚举', () => {
  assert.equal(scopeText('future_scope'), 'future_scope');
});
