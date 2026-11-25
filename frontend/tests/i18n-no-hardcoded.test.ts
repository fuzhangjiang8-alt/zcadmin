// 回归检查：.vue 的模板与脚本（注释除外）不允许出现硬编码中文。
//
// 为什么放在单测里：文案规范靠人 review 容易漏，CI 里跑一遍成本极低；
// 注释允许中文，所以先剔除 HTML 注释、行注释与块注释再做匹配。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../src', import.meta.url));
const chinese = /[\u4e00-\u9fa5]/;

function walk(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(path));
    else if (entry.name.endsWith('.vue')) out.push(path);
  }
  return out;
}

function countHardcoded(source: string): number {
  let hits = 0;
  const template = /<template>([\s\S]*?)<\/template>/.exec(source);
  if (template) {
    const body = template[1].replace(/<!--[\s\S]*?-->/g, '');
    hits += (body.match(chinese) ?? []).length;
  }
  const script = /<script[^>]*>([\s\S]*?)<\/script>/.exec(source);
  if (script) {
    const body = script[1].replace(/\/\/[^\n]*/g, '').replace(/\/\*[\s\S]*?\*\//g, '');
    hits += (body.match(chinese) ?? []).length;
  }
  return hits;
}

test('Vue 模板与脚本中无硬编码中文', () => {
  const offenders = walk(root)
    .map((path) => ({ path, hits: countHardcoded(readFileSync(path, 'utf8')) }))
    .filter((item) => item.hits > 0)
    .map((item) => `${relative(root, item.path)} (${item.hits})`);

  assert.deepEqual(offenders, [], `发现硬编码中文：\n${offenders.join('\n')}`);
});
