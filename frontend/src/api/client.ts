// Fetch wrapper: JSON envelope, auth header injection, ETag caching, 401 handling.

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

interface EtagEntry<T> {
  data: T;
  etag: string;
}

export function setUnauthorizedHandler(handler: () => void): void {
  onUnauthorized = handler;
}

// Bounded in-memory cache for ETag-capable GETs (settings, txt content).
const etagCache = new Map<string, EtagEntry<unknown>>();
const ETAG_CACHE_LIMIT = 200;

export interface RequestOptions {
  body?: unknown;
  /** use ETag cache for this GET request */
  cache?: boolean;
  form?: FormData;
  headers?: Record<string, string>;
  query?: Record<string, boolean | number | string | undefined>;
  /** return raw Response (for zip downloads etc.) */
  raw?: boolean;
  tokenKind?: TokenKind;
}

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

  const res = await fetch(url, { body, headers, method });

  if (res.status === 304 && cached) {
    return cached.data as T;
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

function cachePut<T>(key: string, entry: EtagEntry<T>): void {
  if (etagCache.size >= ETAG_CACHE_LIMIT) {
    // Map iterates in insertion order; drop the oldest entry.
    const oldest = etagCache.keys().next();
    if (!oldest.done) etagCache.delete(oldest.value);
  }
  etagCache.set(key, entry);
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
  return new ApiError(res.status, `Request failed (${res.status})`);
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
