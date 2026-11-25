// =============================================================================
// i18n 文案一致性测试
//
// 断言中英文键集合完全一致、且没有空文案；新增页面文案时 CI 会兜底。
// =============================================================================

import { test } from 'node:test';
import assert from 'node:assert/strict';
import zh from '../src/i18n/zh-CN.ts';
import en from '../src/i18n/en-US.ts';

function flatten(obj: Record<string, unknown>, prefix = ''): string[] {
  return Object.entries(obj).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value && typeof value === 'object') {
      return flatten(value as Record<string, unknown>, path);
    }
    return [path];
  });
}

function emptyValues(obj: Record<string, unknown>, prefix = ''): string[] {
  return Object.entries(obj).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value && typeof value === 'object') {
      return emptyValues(value as Record<string, unknown>, path);
    }
    return typeof value === 'string' && value.trim() ? [] : [path];
  });
}

test('中英文键集合一致', () => {
  assert.deepEqual(flatten(en).sort(), flatten(zh).sort());
});

test('没有空文案', () => {
  assert.deepEqual(emptyValues(zh), []);
  assert.deepEqual(emptyValues(en), []);
});
