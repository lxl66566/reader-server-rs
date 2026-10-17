// E2E 辅助：经 API 造用户/书籍快速铺垫（UI 只测 UI 本身该做的事），
// 会话注入复用前端的 localStorage 存储键（rs_token / rs_locale）。
import { readFileSync } from 'node:fs';

import { expect, type APIRequestContext, type Page } from '@playwright/test';

const API = '/api/v1';

export interface TestUser {
  password: string;
  token: string;
  userId: number;
  username: string;
}

export interface UploadedBook {
  book_id: number;
  format: string;
  title: string;
  /** 仅 pdf 上传响应携带；其余格式为 0 */
  total_pages?: number;
}

// 服务端统一响应包装
interface ApiResponse<T> {
  code: number;
  data: T;
  message: string;
}

interface AuthData {
  token: string;
  user_id: number;
  username: string;
}

interface BookProgress {
  locator: null | string;
  percent: number;
}

/** 服务端 ws 下行帧（仅声明测试用到的子集） */
interface WsFrame {
  message?: string;
  type: string;
  updated?: boolean;
}

/** 经 API 注册专属测试用户 */
export async function createUser(
  request: APIRequestContext,
  prefix: string,
): Promise<TestUser> {
  const username = uniqueUsername(prefix);
  const password = `pw-${username}`;
  const res = await fetchWithRetry(() =>
    request.post(`${API}/auth/register`, {
      data: { password, username },
    }),
  );
  expect(res.ok(), `register ${username}: ${await res.text()}`).toBe(true);
  const { data } = (await res.json()) as ApiResponse<AuthData>;
  return { password, token: data.token, userId: data.user_id, username };
}

/** 读取书籍详情（用于轮询进度落库）；429 时退避重试（轮询高频场景） */
export async function getBook(
  request: APIRequestContext,
  token: string,
  bookId: number,
): Promise<BookProgress> {
  let lastText = '';
  for (let attempt = 0; attempt < 5; attempt++) {
    // oxlint-disable-next-line no-await-in-loop
    const res = await request.get(`${API}/books/${bookId}`, {
      headers: { authorization: `Bearer ${token}` },
    });
    if (res.ok()) {
      // oxlint-disable-next-line no-await-in-loop
      const { data } = (await res.json()) as ApiResponse<BookProgress>;
      return data;
    }
    // oxlint-disable-next-line no-await-in-loop
    lastText = await res.text();
    if (res.status() !== 429) break;
    // oxlint-disable-next-line no-await-in-loop
    await new Promise(resolve => setTimeout(resolve, 500));
  }
  throw new Error(`get book ${bookId}: ${lastText}`);
}

/** 读取当前阅读设置 */
export async function getSettings(
  request: APIRequestContext,
  token: string,
): Promise<Record<string, unknown>> {
  const res = await fetchWithRetry(() =>
    request.get(`${API}/reading/settings`, {
      headers: { authorization: `Bearer ${token}` },
    }),
  );
  expect(res.ok(), `get settings: ${await res.text()}`).toBe(true);
  const { data } = (await res.json()) as ApiResponse<Record<string, unknown>>;
  return data;
}

/**
 * 经 WS 心跳把服务端进度播种到指定位置（进度只能经 ws 心跳写入，无 HTTP
 * 端点）。目标取 E2E_PROXY（直连后端，绕开 vite dev 的 ws 代理偶发掐断），
 * 未设时回退 E2E_BASE_URL——与页面同源，经 vite 的 ws 代理转发；两者都
 * 未设才是默认 :3000。播种 device_id 与页面连接不同会先触发设备让位
 * （首跳不落库），收到 updated=true 的 ack 为止。
 */
export async function seedReadingProgress(
  token: string,
  bookId: number,
  locator: number,
  percent: number,
): Promise<void> {
  const origin = (
    process.env.E2E_PROXY ??
    process.env.E2E_BASE_URL ??
    'http://127.0.0.1:3000'
  )
    .replace(/\/+$/, '')
    .replace(/^http/, 'ws');
  let lastErr: unknown;
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      // 串行重试是语义要求：上一次失败后才能发起下一次
      // oxlint-disable-next-line no-await-in-loop
      await seedOnce(origin, token, bookId, locator, percent);
      return;
    } catch (err) {
      lastErr = err;
      // 间隔重试，避免连发撞上服务端尚未释放的旧连接
      // oxlint-disable-next-line no-await-in-loop
      await new Promise(resolve => setTimeout(resolve, 300));
    }
  }
  throw lastErr instanceof Error
    ? lastErr
    : new Error('seedReadingProgress failed');
}

/** 把 token/locale 注入 localStorage：后续页面加载即处于登录态（zh-CN） */
export function seedSession(page: Page, token: string): void {
  void page.addInitScript(sessionToken => {
    localStorage.setItem('rs_locale', 'zh-CN');
    localStorage.setItem('rs_token', sessionToken);
  }, token);
}

/** 生成唯一用户名（≤20 字符，满足服务端用户名规则） */
export function uniqueUsername(prefix: string): string {
  return `e2e_${prefix}_${Math.random().toString(36).slice(2, 8)}`;
}

/** 修改阅读设置（部分字段） */
export async function updateSettings(
  request: APIRequestContext,
  token: string,
  patch: Partial<{
    background_color: string;
    font_size: number;
    pagination_mode: string;
    text_color: string;
    theme: string;
  }>,
): Promise<void> {
  const res = await fetchWithRetry(() =>
    request.put(`${API}/reading/settings`, {
      data: patch,
      headers: { authorization: `Bearer ${token}` },
    }),
  );
  expect(res.ok(), `update settings: ${await res.text()}`).toBe(true);
}

/** 经 API 上传书籍文件 */
export async function uploadBook(
  request: APIRequestContext,
  token: string,
  filePath: string,
  options: {
    chapterPattern?: string;
    isPublic?: boolean;
    title?: string;
  } = {},
): Promise<UploadedBook> {
  const name = filePath.split(/[/\\]/).at(-1) ?? 'book.txt';
  const mimeType = name.endsWith('.epub')
    ? 'application/epub+zip'
    : name.endsWith('.pdf')
      ? 'application/pdf'
      : 'text/plain';
  const res = await fetchWithRetry(() =>
    request.post(`${API}/books/upload`, {
      headers: { authorization: `Bearer ${token}` },
      multipart: {
        chapter_pattern: options.chapterPattern ?? '',
        file: {
          buffer: readFileSync(filePath),
          mimeType,
          name,
        },
        is_public: String(options.isPublic ?? false),
        title: options.title ?? '',
      },
    }),
  );
  expect(res.ok(), `upload ${name}: ${await res.text()}`).toBe(true);
  const { data } = (await res.json()) as ApiResponse<UploadedBook>;
  return data;
}

/** 等待心跳把阅读进度写到服务端，返回已保存的进度 */
export async function waitForSavedProgress(
  request: APIRequestContext,
  token: string,
  bookId: number,
  timeout = 20_000,
): Promise<{ locator: number; percent: number }> {
  const saved = { locator: 0, percent: 0 };
  await expect
    .poll(
      async () => {
        const book = await getBook(request, token, bookId);
        saved.locator = Number(book.locator ?? 0);
        saved.percent = book.percent;
        return saved.percent;
      },
      { intervals: [1000], timeout },
    )
    .toBeGreaterThan(0);
  return saved;
}

/**
 * 429 退避重试：服务端有全局 IP 限流（10 rps / burst 30），多 worker 同时
 * 建号+上传+开页会瞬间超 burst。仅对幂等/被拒即无副作用的请求使用。
 */
async function fetchWithRetry(
  call: () => Promise<import('@playwright/test').APIResponse>,
): Promise<import('@playwright/test').APIResponse> {
  let res = await call();
  for (let attempt = 0; attempt < 5 && res.status() === 429; attempt++) {
    // oxlint-disable-next-line no-await-in-loop
    await new Promise(resolve => setTimeout(resolve, 500));
    // oxlint-disable-next-line no-await-in-loop
    res = await call();
  }
  return res;
}

/** 单次播种尝试：subscribe + 心跳重发至 updated=true（首跳设备让位不落库） */
function seedOnce(
  wsOrigin: string,
  token: string,
  bookId: number,
  locator: number,
  percent: number,
): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    const ws = new WebSocket(
      `${wsOrigin}/api/v1/reading/ws?token=${encodeURIComponent(token)}`,
    );
    const heartbeat = () => {
      ws.send(
        JSON.stringify({
          book_id: bookId,
          locator: String(locator),
          percent,
          type: 'heartbeat',
        }),
      );
    };
    const timer = setTimeout(() => {
      ws.close();
      reject(new Error('seedReadingProgress timeout'));
    }, 5000);
    const fail = (reason: string) => {
      clearTimeout(timer);
      ws.close();
      reject(new Error(reason));
    };
    ws.onerror = () => {
      fail('seed ws connect failed');
    };
    ws.onopen = () => {
      ws.send(
        JSON.stringify({
          book_id: bookId,
          device_id: 'e2e-seed',
          type: 'subscribe',
        }),
      );
    };
    // vite dev 代理在页面 ws 连接更替时偶发直接掐断升级连接（1006），
    // 视为可重试失败而不是干等超时
    ws.onclose = () => {
      fail('seed ws closed before updated ack');
    };
    ws.onmessage = (evt: MessageEvent) => {
      const frame = JSON.parse(String(evt.data)) as WsFrame;
      if (frame.type === 'subscribed') heartbeat();
      else if (frame.type === 'heartbeat_ack') {
        if (frame.updated) {
          clearTimeout(timer);
          ws.onclose = null;
          ws.close();
          resolve();
        } else heartbeat();
      } else if (frame.type === 'error') {
        fail(`seed ws error: ${frame.message ?? ''}`);
      }
    };
  });
}
