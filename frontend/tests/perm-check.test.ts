// =============================================================================
// 权限判断单元测试
//
// 运行：npm test（Node 内置 test runner + 类型擦除，不引入额外依赖）。
// =============================================================================

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { hasPermission } from '../src/perm-check.ts';

test('超管通配符放行任意权限点', () => {
  assert.equal(hasPermission(['*'], 'system:user:add'), true);
  assert.equal(hasPermission(['*', 'crm:customer:list'], 'anything'), true);
});

test('精确匹配权限点', () => {
  assert.equal(hasPermission(['system:user:list'], 'system:user:list'), true);
  assert.equal(hasPermission(['system:user:list'], 'system:user:add'), false);
});

test('空集合与空权限点', () => {
  assert.equal(hasPermission([], 'system:user:list'), false);
  assert.equal(hasPermission(null, 'system:user:list'), false);
  assert.equal(hasPermission(undefined, 'system:user:list'), false);
  assert.equal(hasPermission(['*'], ''), false);
});

test('大小写与前后缀不模糊匹配', () => {
  assert.equal(hasPermission(['System:User:List'], 'system:user:list'), false);
  assert.equal(hasPermission(['system:user:list:extra'], 'system:user:list'), false);
});
