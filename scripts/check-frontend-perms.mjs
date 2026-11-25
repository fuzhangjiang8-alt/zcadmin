#!/usr/bin/env node
// =============================================================================
// 前端按钮权限扫描：含敏感动作的 a-button 必须带 v-perm（或位于带 v-perm 的 a-upload 内）。
// 允许列表用于明确的自助操作（个人令牌、我的订阅等）。
//
// 为什么不用单条正则：
//   `/<a-button\b([^>]*)>/` 在属性值含 `>` 时会提前截断
//   （例如 `v-if="importResult.errors.length > 0"`），
//   把后续属性当成按钮文本，产生「缺少 v-perm」的误报（CI 会因此必红）。
//   这里改为逐字符扫描，跳过引号内的 `>`。
// =============================================================================

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const dirs = [path.join(root, 'frontend/src/views'), path.join(root, 'frontend/src/plugins')];
const verbs = /新建|新增|编辑|删除|导出|导入|上传|回滚|命名|清理|重投|强制|吊销/;
const allow = new Set([
  'SessionsView.vue:新建令牌',
  'SessionsView.vue:吊销',
  'AuditLogsView.vue:删除', // 规则编辑器行内删除，入口已受 system:audit:alert 控制
  'ExportsView.vue:删除', // 导出任务 owner 语义
]);

function walk(dir) {
  if (!fs.existsSync(dir)) return [];
  const out = [];
  for (const name of fs.readdirSync(dir)) {
    const p = path.join(dir, name);
    const st = fs.statSync(p);
    if (st.isDirectory()) out.push(...walk(p));
    else if (name.endsWith('.vue')) out.push(p);
  }
  return out;
}

/**
 * 提取模板中的全部 `<a-button ...>` 元素。
 * 返回 { attrs, inner, index } 数组；index 用于回看前文判断是否在 a-upload 内。
 */
function findButtons(src) {
  const out = [];
  const open = /<a-button\b/g;
  let m;
  while ((m = open.exec(src))) {
    let i = open.lastIndex;
    let quote = null;
    for (; i < src.length; i += 1) {
      const ch = src[i];
      if (quote) {
        if (ch === quote) quote = null;
      } else if (ch === '"' || ch === "'") {
        quote = ch;
      } else if (ch === '>') {
        break;
      }
    }
    if (i >= src.length) break;
    const attrs = src.slice(open.lastIndex, i);
    const selfClosing = src[i - 1] === '/';
    let inner = '';
    let end = i + 1;
    if (!selfClosing) {
      const close = src.indexOf('</a-button>', i + 1);
      if (close === -1) continue;
      inner = src.slice(i + 1, close);
      end = close + '</a-button>'.length;
    }
    out.push({ attrs, inner, index: m.index });
    open.lastIndex = end;
  }
  return out;
}

const problems = [];
for (const file of dirs.flatMap(walk)) {
  const src = fs.readFileSync(file, 'utf8');
  const base = path.basename(file);
  for (const { attrs, inner, index } of findButtons(src)) {
    const text = inner.replace(/<[^>]+>/g, '').trim();
    if (!verbs.test(text)) continue;
    const before = src.slice(Math.max(0, index - 300), index);
    const inUpload = /<a-upload\b[^>]*v-perm[^>]*>[\s\S]*$/.test(before);
    if (attrs.includes('v-perm') || inUpload) continue;
    const key = `${base}:${text}`;
    if (allow.has(key)) continue;
    problems.push(`${path.relative(root, file)}: ${text}`);
  }
}

if (problems.length > 0) {
  console.error('缺少 v-perm 的按钮:');
  for (const p of problems) console.error('  - ' + p);
  process.exit(1);
}
console.log('前端按钮权限扫描通过');
