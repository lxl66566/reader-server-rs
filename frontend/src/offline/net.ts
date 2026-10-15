// 断网判定与"网络优先、写穿缓存、断网回退"的统一读取模式。

import { createSignal } from 'solid-js';

import { ApiError } from '../api/client';

/** navigator.onLine 跟踪信号：online/offline 事件驱动 */
export function createOnlineSignal(): () => boolean {
  const [online, setOnline] = createSignal(navigator.onLine);
  window.addEventListener('online', () => setOnline(true));
  window.addEventListener('offline', () => setOnline(false));
  return online;
}

/** fetch 失败被归一化为 status 0 的 ApiError（超时/中止除外） */
export function isNetworkError(err: unknown): boolean {
  return err instanceof ApiError && err.status === 0;
}

/** 本地存储配额耗尽（IDB put 抛 QuotaExceededError） */
export function isQuotaError(err: unknown): boolean {
  return err instanceof DOMException && err.name === 'QuotaExceededError';
}
