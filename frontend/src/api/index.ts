import { api } from './client';
import type {
  AdminItem,
  AdminUserItem,
  Annotation,
  AnnotationKind,
  AuditLogItem,
  AuthResult,
  Book,
  BookDetail,
  Bookmark,
  Chapter,
  InviteCodeItem,
  PublicBook,
  ReadingSettings,
  ReadingStats,
  SearchScope,
  UserInfo,
} from './types';

// Endpoint functions grouped by resource.

export * from './client';
export * from './types';

export interface Page<T> {
  books: T[];
  total: number;
}

export const authApi = {
  adminLogin: (username: string, password: string) =>
    api.post<{ admin_token: string }>('/api/v1/auth/admin/login', {
      body: { password, username },
      // 管理员凭据：401 不应触发全局 user 会话清理
      tokenKind: 'admin',
    }),
  adminSetup: (username: string, password: string) =>
    api.post<{ admin_token: string }>('/api/v1/auth/admin/setup', {
      body: { password, username },
      tokenKind: 'admin',
    }),
  changePassword: (oldPassword: string, newPassword: string) =>
    api.post<Record<string, never>>('/api/v1/auth/change_password', {
      body: { new_password: newPassword, old_password: oldPassword },
    }),
  deleteAccount: (password: string) =>
    api.delete<Record<string, never>>('/api/v1/auth/account', {
      body: { password },
    }),
  exportData: () => api.get<Response>('/api/v1/auth/export', { raw: true }),
  login: (username: string, password: string) =>
    api.post<AuthResult>('/api/v1/auth/login', {
      body: { password, username },
    }),
  register: (username: string, password: string, inviteCode?: string) =>
    api.post<AuthResult>('/api/v1/auth/register', {
      body: { invite_code: inviteCode, password, username },
    }),
  userInfo: () => api.get<UserInfo>('/api/v1/auth/user_info'),
};

export const bookApi = {
  content: (bookId: number, position: number, length: number) =>
    api.get<{ content: string; next_position: number }>(
      `/api/v1/books/${bookId}/content`,
      { cache: true, query: { length, position } },
    ),
  detail: (bookId: number) => api.get<BookDetail>(`/api/v1/books/${bookId}`),
  fetchFile: (bookId: number) =>
    api.get<ArrayBuffer>(`/api/v1/books/${bookId}/file`, {
      // 服务端对文件按内容 hash 返回 ETag：304 时复用缓存，
      // 切换分页模式 / 重进书籍不再重复下载整本文件
      binary: true,
      cache: true,
    }),
  fileUrl: (bookId: number) => `/api/v1/books/${bookId}/file`,
  jumpToChapter: (bookId: number, chapterId: number) =>
    api.get<{ position: number }>(`/api/v1/books/${bookId}/jump_to_chapter`, {
      query: { chapter_id: chapterId },
    }),
  list: (page: number, limit: number) =>
    api.get<Page<Book>>('/api/v1/books', { query: { limit, page } }),
  listPublic: (page: number, limit: number) =>
    api.get<Page<PublicBook>>('/api/v1/books/public', {
      query: { limit, page },
    }),
  randomPublic: (count: number) =>
    api.get<{ books: PublicBook[] }>('/api/v1/books/random_public', {
      query: { count },
    }),
  remove: (bookId: number) =>
    api.delete<Record<string, never>>(`/api/v1/books/${bookId}`),
  search: (q: string, scope: SearchScope, page: number, limit: number) =>
    api.get<Page<Book>>('/api/v1/books/search', {
      query: { limit, page, q, scope },
    }),
  setTags: (bookId: number, tags: string[]) =>
    api.put<{ tags: string[] }>(`/api/v1/books/${bookId}/tags`, {
      body: { tags },
    }),
  tags: (bookId: number) =>
    api.get<{ tags: string[] }>(`/api/v1/books/${bookId}/tags`),
  update: (
    bookId: number,
    patch: { author?: string; is_public?: boolean; title?: string },
  ) =>
    api.put<Record<string, never>>(`/api/v1/books/${bookId}`, { body: patch }),
  upload: (form: FormData, signal?: AbortSignal) =>
    api.post<{ book_id: number } & { chapters: Chapter[] }>(
      '/api/v1/books/upload',
      // large files on slow networks need a longer window
      { form, signal, timeout: 120_000 },
    ),
};

export const markerApi = {
  addAnnotation: (
    bookId: number,
    input: {
      content?: string;
      end_locator?: string;
      end_percent?: number;
      excerpt?: string;
      kind: AnnotationKind;
      start_locator: string;
      start_percent: number;
    },
  ) =>
    api.post<Annotation>(`/api/v1/books/${bookId}/annotations`, {
      body: input,
      headers: deviceHeader(),
    }),
  addBookmark: (
    bookId: number,
    locator: string,
    percent: number,
    label?: string,
  ) =>
    api.post<Bookmark>(`/api/v1/books/${bookId}/bookmarks`, {
      body: { label, locator, percent },
      headers: deviceHeader(),
    }),
  annotations: (bookId: number) =>
    api.get<{ annotations: Annotation[] }>(
      `/api/v1/books/${bookId}/annotations`,
    ),
  bookmarks: (bookId: number) =>
    api.get<{ bookmarks: Bookmark[] }>(`/api/v1/books/${bookId}/bookmarks`),
  removeAnnotation: (bookId: number, annotationId: number) =>
    api.delete<Record<string, never>>(
      `/api/v1/books/${bookId}/annotations/${annotationId}`,
      { headers: deviceHeader() },
    ),
  removeBookmark: (bookId: number, bookmarkId: number) =>
    api.delete<Record<string, never>>(
      `/api/v1/books/${bookId}/bookmarks/${bookmarkId}`,
      { headers: deviceHeader() },
    ),
  updateAnnotation: (
    bookId: number,
    annotationId: number,
    patch: { content?: string; excerpt?: string },
  ) =>
    api.put<Record<string, never>>(
      `/api/v1/books/${bookId}/annotations/${annotationId}`,
      { body: patch, headers: deviceHeader() },
    ),
};

export const readingApi = {
  settings: () =>
    api.get<ReadingSettings>('/api/v1/reading/settings', { cache: true }),
  stats: () => api.get<ReadingStats>('/api/v1/reading/stats'),
  updateSettings: (patch: Partial<ReadingSettings>) =>
    api.put<Record<string, never>>('/api/v1/reading/settings', { body: patch }),
};

export const adminApi = {
  admins: () =>
    api.get<{ admins: AdminItem[] }>('/api/v1/admin/admins', {
      tokenKind: 'admin',
    }),
  auditLogs: (page: number, limit: number) =>
    api.get<{ logs: AuditLogItem[]; total: number }>(
      '/api/v1/admin/audit_logs',
      { query: { limit, page }, tokenKind: 'admin' },
    ),
  backup: () =>
    api.get<Response>('/api/v1/admin/backup', {
      raw: true,
      tokenKind: 'admin',
    }),
  ban: (userId: number) =>
    api.post<Record<string, never>>(`/api/v1/admin/users/${userId}/ban`, {
      tokenKind: 'admin',
    }),
  checkSetup: () => api.get<{ is_setup: boolean }>('/api/v1/admin/check_setup'),
  createAdmin: (username: string, password: string) =>
    api.post<Record<string, never>>('/api/v1/admin/admins', {
      body: { password, username },
      tokenKind: 'admin',
    }),
  createInviteCode: (limitTimes: number, description?: string) =>
    api.post<{ invite_code: string }>('/api/v1/admin/invite_code', {
      body: { description, limit_times: limitTimes },
      tokenKind: 'admin',
    }),
  inviteCodes: () =>
    api.get<{ invite_codes: InviteCodeItem[] }>('/api/v1/admin/invite_codes', {
      tokenKind: 'admin',
    }),
  resetPassword: (userId: number, newPassword: string) =>
    api.post<Record<string, never>>(
      `/api/v1/admin/users/${userId}/reset_password`,
      { body: { new_password: newPassword }, tokenKind: 'admin' },
    ),
  revokeTokens: (userId: number) =>
    api.post<Record<string, never>>(
      `/api/v1/admin/users/${userId}/revoke_tokens`,
      { tokenKind: 'admin' },
    ),
  settings: () =>
    api.get<{ invite_code_required: boolean }>('/api/v1/admin/settings', {
      tokenKind: 'admin',
    }),
  unban: (userId: number) =>
    api.post<Record<string, never>>(`/api/v1/admin/users/${userId}/unban`, {
      tokenKind: 'admin',
    }),
  updateSettings: (inviteCodeRequired: boolean) =>
    api.put<Record<string, never>>('/api/v1/admin/settings', {
      body: { invite_code_required: inviteCodeRequired },
      tokenKind: 'admin',
    }),
  users: () =>
    api.get<{ users: AdminUserItem[] }>('/api/v1/admin/users', {
      tokenKind: 'admin',
    }),
};

// X-Device-Id excludes this client from ws change broadcasts (echo avoidance).
function deviceHeader(): Record<string, string> {
  return { 'X-Device-Id': getDeviceId() };
}

let cachedDeviceId: null | string = null;

// device_id = 浏览器基础 id（localStorage）+ 标签页后缀（sessionStorage）。
// ws 注册表以 (user, book, device_id) 为键、广播按 device_id 排除回声，
// 同浏览器多标签页共享同一 id 会互相顶掉订阅、互相收不到进度同步，
// 因此每个标签页必须互异；基础 id 保留浏览器级的稳定身份
export function getDeviceId(): string {
  if (cachedDeviceId) return cachedDeviceId;
  let base = localStorage.getItem('rs_device_id');
  if (!base) {
    base = randomUuid();
    localStorage.setItem('rs_device_id', base);
  }
  let tab = sessionStorage.getItem('rs_device_tab');
  if (!tab) {
    tab = randomUuid().slice(0, 8);
    sessionStorage.setItem('rs_device_tab', tab);
  }
  cachedDeviceId = `${base}-${tab}`;
  return cachedDeviceId;
}

/** randomUUID 的回退实现：非 secure context（如 http://192.168.x.x）下不可用 */
function randomUuid(): string {
  if (typeof crypto.randomUUID === 'function') return crypto.randomUUID();
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  // 按 RFC 4122 v4 固定版本与变体位（?? 0 仅为通过 noUncheckedIndexedAccess）
  bytes[6] = ((bytes[6] ?? 0) & 0x3f) | 0x40;
  bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
  const hex = Array.from(bytes, b => b.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
