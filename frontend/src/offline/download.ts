// 主动缓存下载：txt 分块并发抓取 + epub/pdf 整本文件落盘。
// 断网回退读取不在此处（见 net.ts 的 staleOnError 与各读取端包装）。

import { bookApi } from '../api';
import type { BookDetail } from '../api/types';
import { TXT_CHUNK_SIZE } from '../reader/textSource';
import { chunksForRange } from './coverage';
import {
  bookRev,
  chunkPositions,
  chunkPut,
  clearBookContent,
  filePut,
  metaPut,
} from './db';
import { isQuotaError } from './net';

/** 同时在途的 chunk 请求数：对齐浏览器对单 origin 的并发连接上限 */
const CHUNK_CONCURRENCY = 4;

export interface DownloadProgress {
  done: number;
  total: number;
}

/** 需要区分用户主动取消与失败，取消不弹错误提示 */
export class DownloadCancelled extends Error {
  constructor() {
    super('offline download cancelled');
    this.name = 'DownloadCancelled';
  }
}

export async function clearBook(bookId: number): Promise<void> {
  await clearBookContent(bookId);
}

/** 下载 epub/pdf 整本文件到缓存 */
export async function downloadBookFile(
  detail: BookDetail,
  onProgress: (received: number) => void,
  signal: AbortSignal,
): Promise<void> {
  const data = await bookApi.fetchFileStreaming(
    detail.book_id,
    onProgress,
    signal,
  );
  await filePut(detail.book_id, data, bookRev(detail));
}

/**
 * 下载 txt 字符范围 [start, end) 的全部分块。已缓存的块跳过。
 * signal 中止抛 DownloadCancelled；配额耗尽原样抛出（调用方甄别提示）。
 */
export async function downloadTxtRange(
  bookId: number,
  start: number,
  end: number,
  totalChars: number,
  signal: AbortSignal,
  onProgress: (progress: DownloadProgress) => void,
): Promise<void> {
  const positions = chunksForRange(start, end, totalChars);
  const existing = new Set(await chunkPositions(bookId));
  const todo = positions.filter(position => !existing.has(position));
  const total = positions.length;
  const done = { count: total - todo.length };
  onProgress({ done: done.count, total });

  // 工作池：每个 worker 经共享游标取件，少量并发对齐浏览器连接上限。
  // aborted 经函数读取：await 后 signal 状态可变，不能被流分析收窄
  let cursor = 0;
  const aborted = () => signal.aborted;
  const worker = async (): Promise<void> => {
    for (;;) {
      if (aborted()) throw new DownloadCancelled();
      const position = todo[cursor];
      cursor += 1;
      if (position === undefined) return;
      // oxlint-disable-next-line no-await-in-loop
      const payload = await bookApi.content(bookId, position, TXT_CHUNK_SIZE);
      if (aborted()) throw new DownloadCancelled();
      try {
        // oxlint-disable-next-line no-await-in-loop
        await chunkPut(bookId, position, payload);
      } catch (err) {
        if (isQuotaError(err)) throw err;
        console.warn('offline chunk write failed', err);
      }
      done.count++;
      onProgress({ done: done.count, total });
    }
  };

  await Promise.all(
    Array.from({ length: Math.min(CHUNK_CONCURRENCY, todo.length) }, worker),
  );
}

/** 缓存书籍详情（下载完成后调用，使离线打开详情/阅读可用） */
export async function saveDetail(detail: BookDetail): Promise<void> {
  try {
    await metaPut(detail.book_id, { detail, savedAt: Date.now() });
  } catch (err) {
    // 详情写失败只影响下次离线打开，不阻断在线流程
    console.warn('offline meta write failed', err);
  }
}
