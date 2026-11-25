// HTTP 客户端回归：覆盖网关错误、响应契约和三种 HeadersInit，使用真实 Response 模拟网络边界。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ApiError, request } from '../src/api/client.ts';

test('请求头合并保留自定义值，JSON 请求带 Cookie', async (t) => {
  for (const headers of [new Headers({ 'X-Test': 'ok' }), [['X-Test', 'ok']], { 'X-Test': 'ok' }]) {
    t.mock.method(globalThis, 'fetch', async (_path: string, init: RequestInit) => {
      const actual = new Headers(init.headers);
      assert.equal(actual.get('X-Test'), 'ok');
      assert.equal(actual.get('Content-Type'), 'application/json');
      assert.equal(init.credentials, 'include');
      return Response.json({ data: 1 });
    });
    assert.equal(await request('/api/test', { method: 'POST', body: '{}', headers }), 1);
    t.mock.restoreAll();
  }
});

test('FormData 由浏览器生成 boundary，显式 Content-Type 得到保留', async (t) => {
  t.mock.method(globalThis, 'fetch', async (_path: string, init: RequestInit) => {
    assert.equal(new Headers(init.headers).get('Content-Type'), null);
    return new Response(null, { status: 204 });
  });
  assert.equal(await request('/upload', { method: 'POST', body: new FormData() }), undefined);
  t.mock.restoreAll();
  t.mock.method(globalThis, 'fetch', async (_path: string, init: RequestInit) => {
    assert.equal(new Headers(init.headers).get('Content-Type'), 'text/plain');
    return Response.json({ data: null });
  });
  assert.equal(await request('/api/test', { body: 'text', headers: { 'Content-Type': 'text/plain' } }), null);
});

test('problem 错误保留业务码，非 JSON 错误保留 HTTP 状态且不展示 HTML', async (t) => {
  const problem = JSON.stringify({ code: 'sys.auth.unauthorized', detail: 'expired' });
  const malformed = JSON.stringify({ code: 123, detail: {} });
  for (const [status, body, code, message] of [
    [401, problem, 'sys.auth.unauthorized', 'expired'],
    [429, 'rate limited', 'sys.internal.unexpected', 'HTTP 429'],
    [502, '<html>private upstream address</html>', 'sys.internal.unexpected', 'HTTP 502'],
    [503, '', 'sys.internal.unexpected', 'HTTP 503'],
    [500, malformed, 'sys.internal.unexpected', 'HTTP 500'],
  ] as const) {
    t.mock.method(globalThis, 'fetch', async () => new Response(body, { status }));
    await assert.rejects(request('/api/test'), (error: unknown) => {
      assert.ok(error instanceof ApiError);
      assert.equal(error.status, status);
      assert.equal(error.code, code);
      assert.equal(error.message, message);
      return true;
    });
    t.mock.restoreAll();
  }
});

test('成功响应缺失 data 或无法解析时显式失败，避免把 undefined 当成功', async (t) => {
  for (const body of ['', '<html>SPA fallback</html>', '{}', 'null', '42']) {
    t.mock.method(globalThis, 'fetch', async () => new Response(body));
    await assert.rejects(request('/api/test'), ApiError);
    t.mock.restoreAll();
  }
});
