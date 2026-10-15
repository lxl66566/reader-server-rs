// 断网回退读取：网络优先 + 小数据写穿缓存 + 断网回退 IDB。
// txt 分块随阅读隐式写穿；epub/pdf 整本文件仅在显式"缓存全本"时落盘。

import { bookApi, markerApi, readingApi, type Page } from '../api';
import type {
  Annotation,
  Book,
  BookDetail,
  Bookmark,
  ReadingSettings,
} from '../api/types';
import {
  bookRev,
  chunkGet,
  chunkPut,
  clearBookContent,
  fileGet,
  kvGet,
  kvPut,
  metaGet,
} from './db';
import { saveDetail } from './download';
import { isNetworkError } from './net';

export interface CachedProgress {
  locator: string;
  percent: number;
  /** true = 尚未成功写入服务端（离线阅读产生），恢复在线后需推送 */
  unpushed: boolean;
}

export interface DetailResult {
  detail: BookDetail;
  /** true = 网络不可达，详情取自本地缓存（等效离线态） */
  fromCache: boolean;
}

export async function fetchAnnotations(bookId: number): Promise<Annotation[]> {
  try {
    const value = await markerApi.annotations(bookId);
    void kvPut(`an:${bookId}`, value.annotations);
    return value.annotations;
  } catch (err) {
    if (!isNetworkError(err)) throw err;
    const cached = await kvGet<Annotation[]>(`an:${bookId}`);
    if (cached) return cached;
    throw err;
  }
}

/** epub/pdf 整本文件读取：断网回退显式缓存（未缓存时抛回原错误） */
export async function fetchBookFile(bookId: number): Promise<ArrayBuffer> {
  try {
    return await bookApi.fetchFile(bookId);
  } catch (err) {
    if (!isNetworkError(err)) throw err;
    const cached = await fileGet(bookId);
    if (cached) return cached;
    throw err;
  }
}

export async function fetchBookList(
  page: number,
  limit: number,
): Promise<Page<Book>> {
  try {
    const value = await bookApi.list(page, limit);
    void kvPut(`list:${page}:${limit}`, value);
    return value;
  } catch (err) {
    if (!isNetworkError(err)) throw err;
    const cached = await kvGet<Page<Book>>(`list:${page}:${limit}`);
    if (cached) return cached;
    throw err;
  }
}

export async function fetchBookmarks(bookId: number): Promise<Bookmark[]> {
  try {
    const value = await markerApi.bookmarks(bookId);
    void kvPut(`bm:${bookId}`, value.bookmarks);
    return value.bookmarks;
  } catch (err) {
    if (!isNetworkError(err)) throw err;
    const cached = await kvGet<Bookmark[]>(`bm:${bookId}`);
    if (cached) return cached;
    throw err;
  }
}

export async function fetchDetail(bookId: number): Promise<DetailResult> {
  let detail: BookDetail;
  try {
    detail = await bookApi.detail(bookId);
  } catch (err) {
    if (!isNetworkError(err)) throw err;
    const cached = await metaGet(bookId);
    if (cached) return { detail: cached.detail, fromCache: true };
    throw err;
  }
  // 统计特征变化（重传/重切章）说明旧内容缓存已失效，整体作废后重建
  const cached = await metaGet(bookId);
  if (cached && bookRev(cached.detail) !== bookRev(detail)) {
    await clearBookContent(bookId);
  }
  await saveDetail(detail);
  return { detail, fromCache: false };
}

export async function fetchSettings(): Promise<null | ReadingSettings> {
  try {
    const value = await readingApi.settings();
    void kvPut('settings', value);
    return value;
  } catch (err) {
    if (!isNetworkError(err)) throw err;
    return kvGet<ReadingSettings>('settings');
  }
}

export async function markProgressPushed(bookId: number): Promise<void> {
  const cached = await kvGet<CachedProgress>(`p:${bookId}`);
  // 仅在确有未推送进度时落盘，避免每秒心跳都写 IDB
  if (cached?.unpushed) {
    await kvPut(`p:${bookId}`, { ...cached, unpushed: false });
  }
}

/** TextSource 的 loader 包装：成功写穿、断网回退缓存 */
export function offlineContentLoader(bookId: number) {
  return async (
    position: number,
    length: number,
  ): Promise<{ content: string; next_position: number }> => {
    try {
      const payload = await bookApi.content(bookId, position, length);
      // 写穿：读过即缓存，断网续读当前书；失败（配额等）仅告警
      chunkPut(bookId, position, payload).catch((err: unknown) => {
        console.warn('offline chunk write failed', err);
      });
      return payload;
    } catch (err) {
      if (!isNetworkError(err)) throw err;
      const cached = await chunkGet(bookId, position);
      if (cached) return cached;
      throw err;
    }
  };
}

export function persistSettings(settings: ReadingSettings): void {
  void kvPut('settings', settings);
}

export async function readProgress(
  bookId: number,
): Promise<CachedProgress | null> {
  return kvGet<CachedProgress>(`p:${bookId}`);
}

export async function saveProgress(
  bookId: number,
  locator: string,
  percent: number,
): Promise<void> {
  await kvPut(`p:${bookId}`, { locator, percent, unpushed: true });
}
