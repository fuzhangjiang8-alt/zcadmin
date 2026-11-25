#!/usr/bin/env node
// =============================================================================
// 从迁移 SQL / 插件 PermissionDef 生成权限码常量。
// 用法：
//   node scripts/gen-perm-codes.mjs          # 写入
//   node scripts/gen-perm-codes.mjs --check  # 校验是否最新
// =============================================================================

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const check = process.argv.includes('--check');

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

function collect(prefix) {
  const codes = new Set();
  for (const file of walk(path.join(root, 'crates/kernel/migrations'), '.sql')) {
    const src = fs.readFileSync(file, 'utf8');
    for (const m of src.matchAll(/'([a-z][a-z0-9_-]*:[a-z][a-z0-9_-]*:[a-z][a-z0-9_-]*)'/g)) {
      if (m[1].startsWith(prefix)) codes.add(m[1]);
    }
  }
  for (const file of walk(path.join(root, 'plugins'), '.rs')) {
    const src = fs.readFileSync(file, 'utf8');
    for (const m of src.matchAll(/code:\s*"([^"]+)"/g)) {
      if (m[1].startsWith(prefix)) codes.add(m[1]);
    }
  }
  return [...codes].sort();
}

const constName = (code) => code.split(':').slice(1).join('_').toUpperCase();

function rustConstants(codes) {
  return codes.map((code) => `/// \`${code}\`\npub const ${constName(code)}: &str = "${code}";\n`).join('\n');
}

const systemCodes = collect('system:');
const crmCodes = collect('crm:');
const workflowCodes = collect('workflow:');

const systemFile = path.join(root, 'crates/kernel/src/perms.rs');
const systemContent = `//! 系统内置权限码常量（由 scripts/gen-perm-codes.mjs 生成，请勿手改）。\n\n${rustConstants(systemCodes)}`;

const crmFile = path.join(root, 'plugins/crm/src/perm.rs');
const crmExisting = fs.readFileSync(crmFile, 'utf8');
const crmHeader = crmExisting.split('// 权限码常量')[0].trimEnd();
const crmContent = `${crmHeader}\n\n// 权限码常量（由 scripts/gen-perm-codes.mjs 生成，请勿手改）\n${rustConstants(crmCodes)}`;

const workflowFile = path.join(root, 'plugins/workflow/src/perm.rs');
const workflowExisting = fs.readFileSync(workflowFile, 'utf8');
const workflowHeader = workflowExisting.split('// 权限码常量')[0].trimEnd();
const workflowContent = `${workflowHeader}\n\n// 权限码常量（由 scripts/gen-perm-codes.mjs 生成，请勿手改）\n${rustConstants(workflowCodes)}`;

const tsFile = path.join(root, 'frontend/src/permissions.ts');
const allCodes = [...systemCodes, ...crmCodes, ...workflowCodes].sort();
const tsContent = `// 由 scripts/gen-perm-codes.mjs 生成，请勿手改。\nexport const PERMS = {\n${allCodes
  .map((code) => `  ${constName(code)}: '${code}',`)
  .join('\n')}\n} as const;\n`;

const updates = [
  [systemFile, systemContent],
  [crmFile, crmContent],
  [workflowFile, workflowContent],
  [tsFile, tsContent],
];

let stale = false;
for (const [file, content] of updates) {
  if (check) {
    const current = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : '';
    if (current !== content) {
      console.error(`权限常量不是最新: ${path.relative(root, file)}`);
      stale = true;
    }
  } else {
    fs.writeFileSync(file, content);
  }
}

if (check && stale) process.exit(1);
console.log(
  check
    ? '权限常量已是最新'
    : `已生成 ${systemCodes.length} 个 system、${crmCodes.length} 个 crm、${workflowCodes.length} 个 workflow 常量`,
);
