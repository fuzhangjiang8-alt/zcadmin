#!/usr/bin/env node
// =============================================================================
// 错误码校验：contracts/errors.json 与后端 AppError::code() 必须完全一致。
//
// 为什么需要：contracts/errors.json 是前端文案映射与 SDK 的唯一来源。
// 后端新增/改名字面量而忘了同步契约时，前端会显示 "unknown error"，
// 这类漂移在运行时才暴露，代价很高。
//
// 校验方向：
//   - 代码里有、契约里没有 → **错误**（前端无法翻译，新增错误码必须补契约）
//   - 契约里有、代码里没有 → **告警**（可能是预留条目或历史残留）
//
// 为什么残留只告警不报错：错误码契约遵循「只追加、不改语义」，
// 删除一个已发布的码会让可能依赖它的旧前端/第三方 SDK 失去文案。
// 但残留本身要被看见，所以每次 CI 都会打印出来。
//
// 例外：契约中的 "$comment" 是元数据，不参与比较。
// =============================================================================

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const errorRs = path.join(root, 'crates/kernel/src/error.rs');
const contractFile = path.join(root, 'contracts/errors.json');

const source = fs.readFileSync(errorRs, 'utf8');

// 只抓 `code()` 方法体里的字面量：形如 `=> "sys.auth.unauthorized"`。
// 用 `code()` 函数的边界截取，避免把注释或其他字符串误当成错误码。
const fnStart = source.indexOf('pub fn code(&self)');
if (fnStart < 0) {
  console.error('❌ 未找到 AppError::code()，错误码校验无法执行');
  process.exit(1);
}
const fnEnd = source.indexOf('\n    }', fnStart);
const body = source.slice(fnStart, fnEnd < 0 ? source.length : fnEnd);
const codes = new Set([...body.matchAll(/=>\s*"([a-z][a-z0-9_.]+)"/g)].map((m) => m[1]));

const contract = JSON.parse(fs.readFileSync(contractFile, 'utf8'));
const declared = new Set(Object.keys(contract).filter((k) => !k.startsWith('$')));

const missing = [...codes].filter((c) => !declared.has(c)).sort();
const orphan = [...declared].filter((c) => !codes.has(c)).sort();

let failed = false;
if (missing.length > 0) {
  failed = true;
  console.error('❌ 以下错误码在代码中存在，但 contracts/errors.json 缺失：');
  for (const code of missing) console.error(`   - ${code}`);
}
if (orphan.length > 0) {
  console.warn('⚠️  以下错误码在 contracts/errors.json 中存在，但后端当前不会返回：');
  for (const code of orphan) console.warn(`   - ${code}`);
  console.warn('   （保留是安全的：契约遵循只追加原则；确认无调用方后可在下次大版本移除）');
}

// 附加校验：每个条目必须有 http 与中英文文案，否则前端无法渲染。
for (const key of declared) {
  const entry = contract[key];
  for (const field of ['http', 'zh-CN', 'en-US', 'action']) {
    if (entry[field] === undefined) {
      failed = true;
      console.error(`❌ ${key} 缺少字段 "${field}"`);
    }
  }
}

if (failed) {
  console.error('\n请同步 contracts/errors.json 与 crates/kernel/src/error.rs');
  process.exit(1);
}

console.log(`✅ 错误码契约一致（${codes.size} 个）`);
