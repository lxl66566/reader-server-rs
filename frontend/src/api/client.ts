// Fetch wrapper: JSON envelope, auth header injection, ETag caching, 401 handling.

import { translateGlobal } from '../i18n';
import type { ApiResponse } from './types';

export type TokenKind = 'admin' | 'user';

export class ApiError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

const TOKEN_KEYS: Record<TokenKind, string> = {
  admin: 'rs_admin_token',
  user: 'rs_token',
};

export function getToken(kind: TokenKind): null | string {
  return localStorage.getItem(TOKEN_KEYS[kind]);
}

export function setToken(kind: TokenKind, token: null | string): void {
  if (token === null) {
    localStorage.removeItem(TOKEN_KEYS[kind]);
  } else {
    localStorage.setItem(TOKEN_KEYS[kind], token);
  }
}

// Called when the server rejects a user token (expired / revoked / banned).
let onUnauthorized: (() => void) | null = null;

interface CacheEntry {
  /** 二进制条目的字节量（内存预算用）；JSON 条目为 undefined */
  bytes?: number;
  data: unknown;
  etag: string;
}

export function setUnauthorizedHandler(handler: () => void): void {
  onUnauthorized = handler;
}

// Bounded in-memory cache for ETag-capable GETs (settings, txt content).
const etagCache = new Map<string, CacheEntry>();
const ETAG_CACHE_LIMIT = 200;
// 二进制条目（整本书文件）的内存预算，超出后从最旧条目开始淘汰
const ETAG_BINARY_BUDGET_BYTES = 128 * 1024 * 1024;
let etagBinaryBytes = 0;

export interface RequestOptions {
  /** 返回 ArrayBuffer（整本文件等二进制下载）；配合 cache 支持 ETag 复用 */
  binary?: boolean;
  body?: unknown;
  /** use ETag cache for this GET request */
  cache?: boolean;
  form?: FormData;
  headers?: Record<string, string>;
  /** 二进制下载的进度回调（已接收字节数）；仅 binary 模式生效 */
  onDownloadProgress?: (received: number) => void;
  query?: Record<string, boolean | number | string | undefined>;
  /** return raw Response (for zip downloads etc.) */
  raw?: boolean;
  /** 外部中止信号（如用户取消上传），与 timeout 组合生效 */
  signal?: AbortSignal;
  /** timeout in ms (0 = no timeout); default 15s, disabled for raw/binary */
  timeout?: number;
  tokenKind?: TokenKind;
}

const DEFAULT_TIMEOUT_MS = 15_000;

export async function request<T>(
  method: string,
  path: string,
  options: RequestOptions = {},
): Promise<T> {
  const url = buildUrl(path, options.query);
  const headers: Record<string, string> = { ...options.headers };

  const token = getToken(options.tokenKind ?? 'user');
  if (token) headers.Authorization = `Bearer ${token}`;

  const cacheKey = `${method} ${url}`;
  const cached = options.cache ? etagCache.get(cacheKey) : undefined;
  if (cached) headers['If-None-Match'] = cached.etag;

  let body: BodyInit | undefined;
  if (options.form) {
    body = options.form;
  } else if (options.body !== undefined) {
    headers['Content-Type'] = 'application/json';
    body = JSON.stringify(options.body);
  }

  const timeout =
    options.timeout ?? (options.binary || options.raw ? 0 : DEFAULT_TIMEOUT_MS);
  const timeoutSignal = timeout > 0 ? AbortSignal.timeout(timeout) : undefined;
  // 外部中止信号与超时信号合并生效
  const signals = [options.signal, timeoutSignal].filter(
    (s): s is AbortSignal => s !== undefined,
  );
  const signal = signals.length > 0 ? AbortSignal.any(signals) : undefined;

  let res: Response;
  try {
    res = await fetch(url, { body, headers, method, signal });
  } catch (err) {
    // Normalize abort/network failures so callers always get ApiError
    // (otherwise hung requests leave resources spinning forever).
    if (err instanceof DOMException && err.name === 'TimeoutError') {
      throw new ApiError(0, translateGlobal('errorTimeout'));
    }
    if (err instanceof DOMException && err.name === 'AbortError') {
      throw new ApiError(0, translateGlobal('errorAborted'));
    }
    throw new ApiError(0, translateGlobal('errorNetwork'));
  }

  if (res.status === 304 && cached) {
    return cached.data as T;
  }
  if (options.binary) {
    if (!res.ok) throw await toApiError(res);
    const etag = res.headers.get('ETag');
    const data: unknown = await readBody(res, options.onDownloadProgress);
    if (options.cache && etag) {
      cachePut(cacheKey, {
        bytes: (data as ArrayBuffer).byteLength,
        data,
        etag,
      });
    }
    return data as T;
  }
  if (options.raw) {
    if (!res.ok) throw await toApiError(res);
    return res as T;
  }

  const etag = res.headers.get('ETag');

  if (!res.ok) {
    if (res.status === 401 && (options.tokenKind ?? 'user') === 'user') {
      onUnauthorized?.();
    }
    throw await toApiError(res);
  }

  const envelope = (await res.json()) as ApiResponse<T>;
  if (options.cache && etag) {
    cachePut(cacheKey, { data: envelope.data, etag });
  }
  return envelope.data;
}

function buildUrl(path: string, query?: RequestOptions['query']): string {
  if (!query) return path;
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined) params.set(key, String(value));
  }
  const qs = params.toString();
  return qs ? `${path}?${qs}` : path;
}

function cachePut(key: string, entry: CacheEntry): void {
  // 同 key 覆盖时先归还旧条目的字节量，避免重复累计
  const prev = etagCache.get(key);
  if (prev?.bytes !== undefined) etagBinaryBytes -= prev.bytes;
  if (entry.bytes !== undefined) etagBinaryBytes += entry.bytes;
  etagCache.set(key, entry);
  // Map 按插入序迭代：条数或二进制预算超限时从最旧条目开始淘汰；
  // 始终保留至少一条（单文件不可能超过预算：上传大小有服务端上限）
  while (
    (etagCache.size > ETAG_CACHE_LIMIT ||
      etagBinaryBytes > ETAG_BINARY_BUDGET_BYTES) &&
    etagCache.size > 1
  ) {
    const oldest = etagCache.keys().next();
    if (oldest.done) break;
    const evicted = etagCache.get(oldest.value);
    etagCache.delete(oldest.value);
    if (evicted?.bytes !== undefined) etagBinaryBytes -= evicted.bytes;
  }
}

/** 读响应体为 ArrayBuffer；带进度回调时走流式读取逐段计数 */
async function readBody(
  res: Response,
  onProgress?: (received: number) => void,
): Promise<ArrayBuffer> {
  if (!onProgress || !res.body) return res.arrayBuffer();
  const reader = res.body.getReader();
  const parts: Uint8Array[] = [];
  let received = 0;
  // 流式读取天然逐段 await
  for (;;) {
    // oxlint-disable-next-line no-await-in-loop
    const { done, value } = await reader.read();
    if (done) break;
    parts.push(value);
    received += value.byteLength;
    onProgress(received);
  }
  const out = new Uint8Array(received);
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.byteLength;
  }
  return out.buffer;
}

async function toApiError(res: Response): Promise<ApiError> {
  try {
    const body = (await res.json()) as Partial<ApiResponse<unknown>>;
    if (typeof body.message === 'string' && body.message) {
      return new ApiError(res.status, body.message);
    }
  } catch {
    // non-JSON error body; fall through to generic message
  }
  return new ApiError(
    res.status,
    translateGlobal('errorRequestFailed', { status: res.status }),
  );
}

export const api = {
  delete: <T>(path: string, options?: RequestOptions) =>
    request<T>('DELETE', path, options),
  get: <T>(path: string, options?: RequestOptions) =>
    request<T>('GET', path, options),
  post: <T>(path: string, options?: RequestOptions) =>
    request<T>('POST', path, options),
  put: <T>(path: string, options?: RequestOptions) =>
    request<T>('PUT', path, options),
};
