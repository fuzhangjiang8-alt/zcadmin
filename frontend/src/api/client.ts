// =============================================================================
// HTTP 客户端
//
// 设计要点：
// 1. 统一走 Cookie 会话（credentials: 'include'），前端不碰令牌 —— 防 XSS 窃取。
// 2. 统一解包 { data }，业务代码只拿数据。
// 3. 统一把 problem+json 转成 ApiError，页面只需 catch 后展示 message。
// 4. 401 统一跳登录（由 router 处理，这里只抛错）。
// =============================================================================

/** 后端统一成功响应结构。 */
interface ApiResponse<T> {
  data: T;
}

/** 后端 problem+json 错误结构（RFC 9457）。 */
interface ProblemDetails {
  type?: string;
  title?: string;
  status?: number;
  code?: string;
  detail?: string;
}

/** 业务侧统一异常。 */
export class ApiError extends Error {
  readonly code: string;
  readonly status: number;

  /** 保留 HTTP 状态和机器码，供页面决定提示或跳转。 */
  constructor(
    /** 机器可读错误码，例如 sys.auth.unauthorized */
    code: string,
    /** 人类可读描述 */
    message: string,
    /** HTTP 状态码 */
    status: number,
  ) {
    super(message);
    this.name = 'ApiError';
    this.code = code;
    this.status = status;
  }
}

/** 发起 Cookie 请求；代理错误和异常响应均保留 HTTP 状态，不向页面泄漏 HTML。 */
export async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  // HeadersInit 还可能是 Headers 或元组数组，直接展开会丢失调用方请求头。
  const headers = new Headers(init.headers);
  if (typeof init.body === 'string' && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json');
  }
  const response = await fetch(path, {
    ...init,
    credentials: 'include',
    headers,
  });

  // 204 No Content：没有响应体，直接返回 undefined
  if (response.status === 204) {
    return undefined as T;
  }

  const text = await response.text();
  let payload: unknown = null;
  try {
    payload = text ? JSON.parse(text) : null;
  } catch {
    // 网关的 HTML / 纯文本错误没有 problem 结构，仍需保留 429/502/503 等状态。
  }

  if (!response.ok) {
    const problem = (payload && typeof payload === 'object' ? payload : {}) as ProblemDetails;
    throw new ApiError(
      typeof problem.code === 'string' ? problem.code : 'sys.internal.unexpected',
      typeof problem.detail === 'string' ? problem.detail
        : typeof problem.title === 'string' ? problem.title : `HTTP ${response.status}`,
      response.status,
    );
  }

  if (!payload || typeof payload !== 'object' || !Object.hasOwn(payload, 'data')) {
    throw new ApiError('sys.internal.unexpected', `HTTP ${response.status}`, response.status);
  }
  return (payload as ApiResponse<T>).data;
}

/** 便捷方法。 */
export const http = {
  get: <T>(path: string) => request<T>(path),
  post: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: 'POST', body: body === undefined ? undefined : JSON.stringify(body) }),
  put: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: 'PUT', body: body === undefined ? undefined : JSON.stringify(body) }),
  patch: <T>(path: string, body?: unknown) =>
    request<T>(path, { method: 'PATCH', body: body === undefined ? undefined : JSON.stringify(body) }),
  delete: <T>(path: string) => request<T>(path, { method: 'DELETE' }),
};
