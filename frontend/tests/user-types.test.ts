// =============================================================================
// 用户域纯函数测试
// =============================================================================

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { flattenDepts } from '../src/features/users/types.ts';

test('部门树拍平保留层级缩进', () => {
  const tree = [
    {
      id: 'a',
      name: '总部',
      children: [
        { id: 'a1', name: '研发', children: [] },
        { id: 'a2', name: '销售', children: [{ id: 'a21', name: '华东', children: [] }] },
      ],
    },
  ];
  const options = flattenDepts(tree);
  assert.deepEqual(
    options.map((o) => o.value),
    ['a', 'a1', 'a2', 'a21'],
  );
  assert.equal(options[0].label, '总部');
  assert.equal(options[1].label, '　研发');
  assert.equal(options[3].label, '　　华东');
});

test('空树返回空数组', () => {
  assert.deepEqual(flattenDepts([]), []);
});
