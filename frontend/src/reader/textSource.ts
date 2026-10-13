// Chunked txt content loader: the server streams 6000-char slices, we cache
// them (client also applies ETag caching) and expose sync slicing.

import { bookApi } from '../api';

export const TXT_CHUNK_SIZE = 6000;

/** 滚动窗口最多保留的 chunk 数，超出后按滚动方向回收远端，避免 DOM 无界增长 */
export const MAX_SCROLL_WINDOW_CHUNKS = 9;

/** 滚动模式已加载的 chunk 窗口 [start, end) */
export interface ScrollWindow {
  /** 末尾 chunk 索引（exclusive） */
  end: number;
  /** 起始 chunk 索引 */
  start: number;
}

interface Chunk {
  /** server-reported char offset right after this chunk */
  next: number;
  text: string;
}

export class TextSource {
  private chunks = new Map<number, Chunk>();
  private loader: (
    position: number,
    length: number,
  ) => Promise<{ content: string; next_position: number }>;
  private pending = new Map<number, Promise<void>>();

  constructor(
    bookId: number,
    readonly totalChars: number,
    loader?: (
      position: number,
      length: number,
    ) => Promise<{ content: string; next_position: number }>,
  ) {
    this.loader =
      loader ??
      ((position, length) => bookApi.content(bookId, position, length));
  }

  chunkStart(offset: number): number {
    return Math.floor(offset / TXT_CHUNK_SIZE) * TXT_CHUNK_SIZE;
  }

  /** Load every chunk intersecting [start, end). */
  async ensureRange(start: number, end: number): Promise<void> {
    const jobs: Promise<void>[] = [];
    for (
      let at = this.chunkStart(start);
      at < end && at < this.totalChars;
      at += TXT_CHUNK_SIZE
    ) {
      if (this.chunks.has(at)) continue;
      jobs.push(this.load(at));
    }
    await Promise.all(jobs);
  }

  has(offset: number): boolean {
    return this.chunks.has(this.chunkStart(offset));
  }

  /** Sync slice; caller must ensureRange first. Tolerates partial coverage. */
  slice(start: number, end: number): string {
    let out = '';
    for (
      let at = this.chunkStart(start);
      at < end && at < this.totalChars;
      at += TXT_CHUNK_SIZE
    ) {
      const chunk = this.chunks.get(at);
      if (!chunk) break;
      const from = Math.max(start - at, 0);
      const to = Math.min(end - at, chunk.text.length);
      out += chunk.text.slice(from, to);
    }
    return out;
  }

  private load(at: number): Promise<void> {
    const existing = this.pending.get(at);
    if (existing) return existing;
    const job = this.loader(at, TXT_CHUNK_SIZE)
      .then(res => {
        this.chunks.set(at, { next: res.next_position, text: res.content });
      })
      .finally(() => {
        this.pending.delete(at);
      });
    this.pending.set(at, job);
    return job;
  }
}

/** 章节的字符范围 [start, end)；无章节信息的书籍退化为整本 */
export function chapterBounds(
  chapters: readonly { position: number }[],
  index: number,
  totalChars: number,
): { end: number; start: number } {
  return {
    end: chapters.at(index + 1)?.position ?? totalChars,
    start: chapters.at(index)?.position ?? 0,
  };
}

/** 最后一个 position <= at 的章节下标（列表按 position 升序；空列表返回 0） */
export function chapterIndexAt(
  chapters: readonly { position: number }[],
  at: number,
): number {
  let best = 0;
  for (const [index, chapter] of chapters.entries()) {
    if (chapter.position > at) break;
    best = index;
  }
  return best;
}

/**
 * 向下扩展一个 chunk。超过上限且 `canDropTop` 时回收顶部 chunk；
 * `canDropTop` 为 false（视口仍停留在顶部 chunk）时允许暂时超限。
 * `maxEnd` 为窗口 end 的上限（exclusive），章节模式下传章节末尾 chunk。
 */
export function extendWindowDown(
  win: ScrollWindow,
  maxEnd: number,
  canDropTop: boolean,
): ScrollWindow {
  const end = Math.min(win.end + 1, maxEnd);
  const start =
    canDropTop && end - win.start > MAX_SCROLL_WINDOW_CHUNKS
      ? win.start + 1
      : win.start;
  return { end, start };
}

// --- 章节范围换算（TXT 章节由后端按行解析，position 为字符偏移） ---

/**
 * 向上扩展一个 chunk。超过上限且 `canDropBottom` 时回收底部 chunk；
 * `canDropBottom` 为 false（视口仍停留在底部 chunk）时允许暂时超限。
 * `minStart` 为窗口 start 的下限，章节模式下传章节起始 chunk。
 */
export function extendWindowUp(
  win: ScrollWindow,
  minStart: number,
  canDropBottom: boolean,
): ScrollWindow {
  const start = Math.max(minStart, win.start - 1);
  const end =
    canDropBottom && win.end - start > MAX_SCROLL_WINDOW_CHUNKS
      ? win.end - 1
      : win.end;
  return { end, start };
}

/**
 * 跳转时的窗口换算：目标 chunk 已在窗口内则原样返回（保持引用相等），
 * 否则重置为以目标为起点的窗口，避免远距离跳转把窗口拉得过大。
 */
export function jumpWindow(
  win: ScrollWindow,
  target: number,
  maxEnd: number,
): ScrollWindow {
  if (target >= win.start && target < win.end) return win;
  return { end: Math.min(target + 1, maxEnd), start: target };
}
