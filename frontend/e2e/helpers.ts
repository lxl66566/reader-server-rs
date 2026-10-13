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

/** 经 API 注册专属测试用户 */
export async function createUser(
  request: APIRequestContext,
  prefix: string,
): Promise<TestUser> {
  const username = uniqueUsername(prefix);
  const password = `pw-${username}`;
  const res = await request.post(`${API}/auth/register`, {
    data: { password, username },
  });
  expect(res.ok(), `register ${username}: ${await res.text()}`).toBe(true);
  const { data } = (await res.json()) as ApiResponse<AuthData>;
  return { password, token: data.token, userId: data.user_id, username };
}

/** 读取书籍详情（用于轮询进度落库） */
export async function getBook(
  request: APIRequestContext,
  token: string,
  bookId: number,
): Promise<BookProgress> {
  const res = await request.get(`${API}/books/${bookId}`, {
    headers: { authorization: `Bearer ${token}` },
  });
  expect(res.ok(), `get book ${bookId}: ${await res.text()}`).toBe(true);
  const { data } = (await res.json()) as ApiResponse<BookProgress>;
  return data;
}

/** 读取当前阅读设置 */
export async function getSettings(
  request: APIRequestContext,
  token: string,
): Promise<Record<string, unknown>> {
  const res = await request.get(`${API}/reading/settings`, {
    headers: { authorization: `Bearer ${token}` },
  });
  expect(res.ok(), `get settings: ${await res.text()}`).toBe(true);
  const { data } = (await res.json()) as ApiResponse<Record<string, unknown>>;
  return data;
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
  const res = await request.put(`${API}/reading/settings`, {
    data: patch,
    headers: { authorization: `Bearer ${token}` },
  });
  expect(res.ok(), `update settings: ${await res.text()}`).toBe(true);
}

/** 经 API 上传书籍文件 */
export async function uploadBook(
  request: APIRequestContext,
  token: string,
  filePath: string,
  options: { isPublic?: boolean; title?: string } = {},
): Promise<UploadedBook> {
  const name = filePath.split('/').at(-1) ?? 'book.txt';
  const res = await request.post(`${API}/books/upload`, {
    headers: { authorization: `Bearer ${token}` },
    multipart: {
      file: {
        buffer: readFileSync(filePath),
        mimeType: name.endsWith('.epub')
          ? 'application/epub+zip'
          : 'text/plain',
        name,
      },
      is_public: String(options.isPublic ?? false),
      title: options.title ?? '',
    },
  });
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
