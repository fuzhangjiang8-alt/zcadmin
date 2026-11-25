#!/usr/bin/env node
// =============================================================================
// 权限码校验：代码中引用的权限必须已在迁移或插件 PermissionDef 中声明。
// =============================================================================

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const known = new Set();

function walk(dir, ext, out = []) {
  if (!fs.existsSync(dir)) return out;
  for (const name of fs.readdirSync(dir)) {
    const p = path.join(dir, name);
    const st = fs.statSync(p);
    if (st.isDirectory()) walk(p, ext, out);
    else if (name.endsWith(ext)) out.push(p);
  }
  return out;
}

// 迁移中的权限声明
for (const file of walk(path.join(root, 'crates/kernel/migrations'), '.sql')) {
  const src = fs.readFileSync(file, 'utf8');
  for (const m of src.matchAll(/'([a-z][a-z0-9_-]*:[a-z][a-z0-9_-]*:[a-z][a-z0-9_-]*)'/g)) {
    known.add(m[1]);
  }
}

// 插件权限声明
for (const file of walk(path.join(root, 'plugins'), '.rs')) {
  const src = fs.readFileSync(file, 'utf8');
  for (const m of src.matchAll(/code:\s*"([^"]+)"/g)) known.add(m[1]);
}

const sources = [
  ...walk(path.join(root, 'crates/kernel/src'), '.rs'),
  ...walk(path.join(root, 'plugins'), '.rs'),
];

const problems = [];
for (const file of sources) {
  const src = fs.readFileSync(file, 'utf8');
  const patterns = [
    /(?:require_perm|perm::require)\(&?user,\s*"([^"]+)"/g,
    /has_permission\("([^"]+)"\)/g,
  ];
  for (const re of patterns) {
    for (const m of src.matchAll(re)) {
      const code = m[1];
      if (code === '*') continue;
      if (!known.has(code)) {
        problems.push(`${path.relative(root, file)}: ${code}`);
      }
    }
  }
}

// 前端 v-perm 引用的权限码
for (const file of walk(path.join(root, 'frontend/src'), '.vue')) {
  const src = fs.readFileSync(file, 'utf8');
  for (const m of src.matchAll(/v-perm="'([^']+)'"/g)) {
    if (!known.has(m[1])) problems.push(`${path.relative(root, file)}: ${m[1]}`);
  }
}

if (problems.length > 0) {
  console.error('引用了未声明的权限码:');
  for (const p of problems) console.error('  - ' + p);
  process.exit(1);
}
console.log(`权限码校验通过（声明 ${known.size} 个）`);
