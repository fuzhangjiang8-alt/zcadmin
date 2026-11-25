#!/usr/bin/env node
// =============================================================================
// 模块权限扫描：接收 CurrentUser 的 handler 必须显式校验权限。
// 允许列表仅用于内部 helper 或按 user_id/tenant_id 收窄的自助接口。
//
// 为什么把 kernel 也纳入扫描：
//   内核里绝大多数写接口同样需要权限点，漏一次 require_perm 就是越权；
//   此前只扫 plugins/*/src，新增内核接口漏校验时 CI 不会报错。
//   kernel 存在一批「我的令牌 / 我的订阅 / 未读通知 / 公开字典」等自助接口，
//   它们按 CurrentUser 的 tenant_id + user_id 收窄，本就不该要求功能权限点，
//   因此用「文件::函数」精确键登记理由，而不是放宽整条规则。
// =============================================================================

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** 插件侧：内部 helper 由调用方先校验。 */
const pluginAllow = new Map([
  ['ensure_customer_visible', '内部可见性 helper，由调用方先校验'],
  ['ensure_contract_visible', '同上：合同父实体可见性 helper'],
  ['insert_row', 'Excel 导入内部函数'],
  // demo 插件的路由探针：已要求登录（CurrentUser），但不读写数据，无需功能权限点。
  ['ping', '插件健康探测：只验登录、不访问数据'],
]);

/**
 * 内核侧自助/内部接口白名单。
 *
 * 键为 `相对路径::函数名`，避免同名函数被误放行；每条都必须写明
 * 「为什么不需要功能权限点」。新增条目意味着新增一个无权限点的写接口，
 * 评审时应确认它确实按 user_id/tenant_id 收窄。
 */
const kernelAllow = new Map([
  ['crates/kernel/src/user_import_parse.rs::validate_row', '导入内部 helper'],
  ['crates/kernel/src/user_import_parse.rs::insert_row', '导入内部 helper'],
  ['crates/kernel/src/auth_session_api.rs::me', '当前用户信息，自助读取'],
  ['crates/kernel/src/auth_session_api.rs::change_password', '本人改密，按 user_id + 原密码校验，自助接口'],
  ['crates/kernel/src/auth_tokens.rs::list_tokens', '个人 API 令牌，按 user_id 收窄'],
  ['crates/kernel/src/auth_tokens.rs::create_token', '个人 API 令牌，按 user_id 收窄'],
  ['crates/kernel/src/auth_tokens.rs::revoke_token', '个人 API 令牌，按 user_id 收窄'],
  ['crates/kernel/src/audit_subscriptions.rs::list_alert_subscriptions', '我的告警订阅，按 user_id 收窄'],
  ['crates/kernel/src/audit_subscriptions.rs::replace_alert_subscriptions', '我的告警订阅，按 user_id 收窄'],
  ['crates/kernel/src/audit_alerts.rs::list_subscribed_alerts', '我的告警列表，按 user_id 收窄'],
  ['crates/kernel/src/audit_subscriptions.rs::get_subscription_config', '我的订阅配置，按 user_id 收窄'],
  ['crates/kernel/src/audit_subscriptions.rs::update_subscription_config', '我的订阅配置，按 user_id 收窄'],
  ['crates/kernel/src/configs.rs::public_configs', '公开配置字典，登录即可读'],
  ['crates/kernel/src/exports_download.rs::download', '导出任务按 tenant_id + user_id 归属校验'],
  ['crates/kernel/src/notices.rs::list_notices', '站内通知按当前用户收窄'],
  ['crates/kernel/src/notices.rs::unread_count', '当前用户未读数'],
  ['crates/kernel/src/notices.rs::mark_read', '当前用户已读标记'],
  ['crates/kernel/src/notices.rs::mark_all_read', '当前用户已读标记'],
  ['crates/kernel/src/system.rs::list_enabled_plugins', '插件清单，登录即可读'],
  ['crates/kernel/src/system_dicts.rs::list_dict_items', '字典项，登录即可读'],
]);

function walk(dir) {
  const out = [];
  for (const name of fs.readdirSync(dir)) {
    const p = path.join(dir, name);
    const st = fs.statSync(p);
    if (st.isDirectory()) out.push(...walk(p));
    else if (name.endsWith('.rs')) out.push(p);
  }
  return out;
}

const groups = [];
for (const plugin of fs.readdirSync(path.join(root, 'plugins'))) {
  const src = path.join(root, 'plugins', plugin, 'src');
  if (fs.existsSync(src)) groups.push({ dir: src, kind: 'plugin' });
}
const kernelSrc = path.join(root, 'crates/kernel/src');
if (fs.existsSync(kernelSrc)) groups.push({ dir: kernelSrc, kind: 'kernel' });

/**
 * 扫描一个 Rust 文件里的 async fn，返回 { name, args, body }。
 *
 * 为什么不用单个正则抓函数体：早期版本用 `[\s\S]*?\n\}` 猜结尾，
 * 遇到紧邻函数或嵌套块会把下一个函数并进本次匹配，导致误报函数名。
 * 这里改为「定位签名 → 从函数体起始大括号做配平」，并跳过字符串与
 * 行注释里的括号（Rust 字符串里出现 `{`/`}` 很常见，如 SQL 与 JSON）。
 */
function scanFunctions(src) {
  const out = [];
  const sigRe = /\basync\s+fn\s+([a-z0-9_]+)\s*\(/g;
  let sig;
  while ((sig = sigRe.exec(src))) {
    const name = sig[1];
    // 参数表：从 '(' 配平到 ')'
    const argsStart = sig.index + sig[0].length;
    const argsEnd = matchingParen(src, argsStart - 1, '(', ')');
    if (argsEnd < 0) continue;
    const args = src.slice(argsStart, argsEnd);

    // 返回类型之后的函数体起始 '{'
    const brace = src.indexOf('{', argsEnd);
    if (brace < 0) continue;
    // 若 '{' 前出现分号，说明这是 trait 声明而非实现，跳过
    if (src.slice(argsEnd, brace).includes(';')) continue;
    const bodyEnd = matchingBrace(src, brace);
    if (bodyEnd < 0) continue;
    out.push({ name, args, body: src.slice(brace + 1, bodyEnd) });
    sigRe.lastIndex = bodyEnd;
  }
  return out;
}

/**
 * 配平圆括号/大括号，忽略字符串字面量、行注释与块注释内的符号。
 *
 * Rust 原始字符串 `r#"..."#` / `r##"..."##` 在 SQL、JSON 模板里很常见，
 * 其中的 `{` `}` 不能参与配平；否则一个 `json!({ ... })` 就会让函数体
 * 提前收尾，后续函数被漏扫。这里按开头的 `#` 个数记录结束分隔符。
 */
function matchDelimiter(src, start, open, close) {
  let depth = 0;
  let inString = false;
  let rawHashes = -1; // >=0 表示正处于原始字符串内，值为 # 的个数
  let inLineComment = false;
  let inBlockComment = 0;
  for (let i = start; i < src.length; i++) {
    const c = src[i];
    if (inLineComment) {
      if (c === '\n') inLineComment = false;
      continue;
    }
    if (inBlockComment > 0) {
      if (c === '/' && src[i - 1] === '*') inBlockComment--;
      else if (c === '*' && src[i + 1] === '/') { inBlockComment++; i++; }
      continue;
    }
    if (rawHashes >= 0) {
      // 结束分隔符是 `"` 紧跟 rawHashes 个 `#`，例如 r#"..."# 对应 `"#`。
      if (c === '"' && src.slice(i + 1, i + 1 + rawHashes) === '#'.repeat(rawHashes)) {
        i += rawHashes;
        rawHashes = -1;
      }
      continue;
    }
    if (inString) {
      if (c === '\\') { i++; continue; }
      if (c === '"') inString = false;
      continue;
    }
    // 原始字符串开始：r#" / r##" / ...；`r` 必须不是标识符的一部分，
    // 否则 `user`/`str` 这类词尾的 r 会被误判成原始字符串起点。
    const prev = src[i - 1] ?? '';
    const identPart = /[A-Za-z0-9_]/.test(prev);
    if (!identPart && c === 'r' && src[i + 1] === '"') { inString = true; i++; continue; }
    if (!identPart && c === 'r') {
      let n = 0;
      while (src[i + 1 + n] === '#') n++;
      if (n > 0 && src[i + 1 + n] === '"') { rawHashes = n; i += n + 1; continue; }
    }
    if (c === '/' && src[i + 1] === '/') { inLineComment = true; i++; continue; }
    if (c === '/' && src[i + 1] === '*') { inBlockComment = 1; i++; continue; }
    if (c === '"') { inString = true; continue; }
    if (c === open) depth++;
    else if (c === close && --depth === 0) return i;
  }
  return -1;
}

function matchingParen(src, start, open, close) {
  return matchDelimiter(src, start, open, close);
}

function matchingBrace(src, start) {
  return matchDelimiter(src, start, '{', '}');
}

const problems = [];
const found = new Set();
let scanned = 0;
for (const { dir, kind } of groups) {
  const files = walk(dir);
  scanned += files.length;
  for (const file of files) {
    const src = fs.readFileSync(file, 'utf8');
    const rel = path.relative(root, file).replace(/\\/g, '/');
    for (const { name, args, body } of scanFunctions(src)) {
      found.add(`${rel}::${name}`);
      // axum 提取器形状是按值 `user: CurrentUser`；`&CurrentUser` 是审计/校验等
      // 内部 helper 的引用参数，不是 HTTP handler，不该纳入权限门禁。
      if (!/\b\w+\s*:\s*CurrentUser\b/.test(args)) continue;
      if (body.includes('require_perm') || body.includes('perm::require') || body.includes('has_permission')) continue;
      if (kind === 'plugin' && pluginAllow.has(name)) continue;
      if (kind === 'kernel' && kernelAllow.has(`${rel}::${name}`)) continue;
      problems.push(`${rel}::${name}`);
    }
  }
}

// 白名单必须指向真实存在的 async fn：函数改名/删除后若留着旧键，
// 等于给未来同名 handler 预留了一个「永远放行」的死角。
const stale = [...kernelAllow.keys()].filter((key) => !found.has(key));
if (stale.length > 0) {
  console.error('内核白名单已失效（源码中找不到对应函数）：');
  for (const p of stale) console.error('  - ' + p);
  process.exit(1);
}

if (problems.length > 0) {
  console.error('缺少权限校验的 handler（自助接口请在脚本中登记理由）：');
  for (const p of problems) console.error('  - ' + p);
  process.exit(1);
}
console.log(`权限扫描通过（${scanned} 个源文件：插件 + 内核）`);
