#!/usr/bin/env node
// =============================================================================
// 租户隔离守护：迁移里「建了策略却没 FORCE」的表必须为零。
//
// 为什么需要这个检查
// ------------------
// PostgreSQL 的 RLS 有个反直觉行为：
//   - ENABLE  ROW LEVEL SECURITY：**表 owner 可以绕过**策略
//   - FORCE   ROW LEVEL SECURITY：owner 也不能绕过
// 而应用连的如果恰好是建表角色（本地开发常见），就等于 RLS 完全没生效。
// 这类问题不会报错，只会静默泄漏跨租户数据 —— 必须靠机器检查。
//
// 判定规则
// --------
// 1. 收集所有 `CREATE POLICY ... ON <table>` 涉及的表 → 这些表「宣称」要隔离；
// 2. 收集所有 `FORCE ROW LEVEL SECURITY` 涉及的表；
// 3. 差集必须为空。
//
// 已知例外（必须在此显式登记，新增例外要写理由）：
//   - 分区子表：策略建在父表上，子表由父表继承，不单独出现 CREATE POLICY；
//   - 本脚本只做静态检查，运行期用 docs/03 §9.4 的 SQL 复核。
// =============================================================================

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** 递归收集 .sql 文件（kernel + 所有插件的 migrations）。 */
function collectSql(dir, out = []) {
  if (!fs.existsSync(dir)) return out;
  for (const name of fs.readdirSync(dir)) {
    const p = path.join(dir, name);
    const st = fs.statSync(p);
    if (st.isDirectory()) collectSql(p, out);
    else if (name.endsWith('.sql')) out.push(p);
  }
  return out;
}

/**
 * 归一化表名：去掉 schema 前缀、引号、空白。
 *
 * 迁移里同时存在 `sys.sys_user` 与 `sys_user` 两种写法（search_path 已设置），
 * 必须统一才能正确比较。
 */
function normalize(name) {
  return name
    .trim()
    .replace(/"/g, '')
    .replace(/^(sys|crm|demo|workflow)\./, '')
    .toLowerCase();
}

const files = [
  ...collectSql(path.join(root, 'crates/kernel/migrations')),
  ...collectSql(path.join(root, 'plugins')),
].filter((f) => f.includes(`${path.sep}migrations${path.sep}`));

/**
 * 去掉 SQL 行注释。
 *
 * 迁移文件的注释里经常出现示例语句（例如 0010 的
 * `-- 回滚：... ALTER TABLE ... FORCE ...`），如果不剥离就会误判为真实 DDL。
 * 这里只处理 `--` 行注释；本项目迁移中没有块注释，也没在字符串里写 `--`。
 */
function stripComments(sql) {
  return sql
    .split('\n')
    .map((line) => {
      const idx = line.indexOf('--');
      return idx >= 0 ? line.slice(0, idx) : line;
    })
    .join('\n');
}

const policyTables = new Map(); // table -> [file]
const forceTables = new Map();

for (const file of files) {
  const src = stripComments(fs.readFileSync(file, 'utf8'));
  const rel = path.relative(root, file);

  for (const m of src.matchAll(/CREATE\s+POLICY\s+\S+\s+ON\s+([a-zA-Z0-9_."]+)/gi)) {
    const table = normalize(m[1]);
    if (!policyTables.has(table)) policyTables.set(table, []);
    policyTables.get(table).push(rel);
  }

  for (const m of src.matchAll(/ALTER\s+TABLE\s+([a-zA-Z0-9_."]+)\s+FORCE\s+ROW\s+LEVEL\s+SECURITY/gi)) {
    const table = normalize(m[1]);
    if (!forceTables.has(table)) forceTables.set(table, []);
    forceTables.get(table).push(rel);
  }
}

// 有策略但没有 FORCE 的表 = 隔离缺口（owner 可绕过）
const missing = [...policyTables.keys()].filter((t) => !forceTables.has(t)).sort();

// 反向：FORCE 了但没有任何策略 —— 该表会完全不可读（除非 owner 且 NO FORCE）。
// 这通常是漏建策略，属于更严重的错误，一并检查。
const forceWithoutPolicy = [...forceTables.keys()]
  .filter((t) => !policyTables.has(t))
  .sort();

let failed = false;

if (missing.length > 0) {
  failed = true;
  console.error('❌ 以下表建了 RLS 策略但没有 FORCE（表 owner 可绕过，跨租户可读）：');
  for (const table of missing) {
    console.error(`   - ${table}   （策略见 ${[...new Set(policyTables.get(table))].join(', ')}）`);
  }
  console.error('   修复：在迁移里补 ALTER TABLE <table> FORCE ROW LEVEL SECURITY;');
}

if (forceWithoutPolicy.length > 0) {
  failed = true;
  console.error('❌ 以下表 FORCE 了 RLS 但没有任何策略（该表将完全不可读）：');
  for (const table of forceWithoutPolicy) {
    console.error(`   - ${table}   （FORCE 见 ${[...new Set(forceTables.get(table))].join(', ')}）`);
  }
}

if (failed) {
  console.error('\n参考：docs/03-数据架构.md §9.4、docs/10-后端代码地图.md §5.2');
  process.exit(1);
}

console.log(
  `✅ 租户隔离迁移一致：${policyTables.size} 张表有策略，且全部已 FORCE RLS`
);
