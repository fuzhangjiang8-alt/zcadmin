// 会话回归：通过实际 store 验证退出和切换账号时的异步响应乱序，不启动浏览器。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { registerHooks } from 'node:module';

// Vite 支持无扩展名 TS 导入；只在本测试进程补齐 Node 的相同解析规则。
const hooks = registerHooks({
  resolve(specifier, context, next) {
    try { return next(specifier, context); }
    catch (error) {
      if (specifier.startsWith('.') && !specifier.endsWith('.ts')) return next(`${specifier}.ts`, context);
      throw error;
    }
  },
});
const { clearSession, loadSession, useSession } = await import('../src/stores/session.ts');
const { queryClient } = await import('../src/api/query-client.ts');
const { loadDict } = await import('../src/composables/useDict.ts');
hooks.deregister();

/** 手动控制返回时刻，确保回归覆盖旧请求后到的真实顺序。 */
function delayed() {
  let resolve!: (value: Response) => void;
  const promise = new Promise<Response>((done) => { resolve = done; });
  return { promise, resolve };
}

/** 返回完整用户结构，身份与权限必须一起切换。 */
function user(username: string) {
  return { userId: username, tenantId: 'tenant', username, permissions: [username], mustChangePassword: false };
}

test('并发加载去重，退出后旧请求不能恢复用户权限', async (t) => {
  clearSession();
  t.after(clearSession);
  const old = delayed();
  const fetch = t.mock.method(globalThis, 'fetch', () => old.promise);
  const first = loadSession();
  assert.equal(loadSession(), first);
  assert.equal(fetch.mock.callCount(), 1);
  clearSession();
  const rejection = assert.rejects(first, { code: 'sys.auth.unauthorized' });
  old.resolve(Response.json({ data: user('old') }));
  await rejection;
  assert.equal(useSession().currentUser.value, null);
  assert.equal(useSession().hasPerm('old'), false);
});

test('旧请求结束不能覆盖新账号，也不能破坏新请求去重', async (t) => {
  clearSession();
  t.after(clearSession);
  const old = delayed();
  const fresh = delayed();
  let calls = 0;
  t.mock.method(globalThis, 'fetch', () => (++calls === 1 ? old.promise : fresh.promise));
  const first = loadSession();
  clearSession();
  const second = loadSession();
  const rejection = assert.rejects(first, { code: 'sys.auth.unauthorized' });
  old.resolve(Response.json({ data: user('old') }));
  await rejection;
  assert.equal(loadSession(), second);
  fresh.resolve(Response.json({ data: user('new') }));
  await second;
  assert.equal(useSession().currentUser.value?.username, 'new');
  assert.equal(useSession().hasPerm('old'), false);
  assert.equal(useSession().hasPerm('new'), true);
  assert.equal(calls, 2);
});

test('退出时清空上一租户的数据缓存和字典，取消的旧查询不能回填', async (t) => {
  clearSession();
  t.after(clearSession);
  queryClient.setQueryData(['audit-logs'], ['tenant-a']);
  const old = delayed();
  t.mock.method(globalThis, 'fetch', () => old.promise);
  const dict = loadDict('notice_type');
  const rejection = assert.rejects(dict);
  clearSession();
  await rejection;
  old.resolve(Response.json({ data: [{ id: 'old', label: 'old', value: 'old', sort: 0, status: 'active' }] }));
  await new Promise((done) => setImmediate(done));
  assert.equal(queryClient.getQueryData(['audit-logs']), undefined);
  assert.equal(queryClient.getQueryData(['system-dict', 'notice_type']), undefined);
  t.mock.restoreAll();
  t.mock.method(globalThis, 'fetch', async () => Response.json({ data: [] }));
  assert.deepEqual(await loadDict('notice_type'), []);
});
