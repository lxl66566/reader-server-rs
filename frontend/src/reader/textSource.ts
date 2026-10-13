// Chunked txt content loader: the server streams 6000-char slices, we cache
// them (client also applies ETag caching) and expose sync slicing.

import { bookApi } from '../api';

export const TXT_CHUNK_SIZE = 6000;

/** 缓存 chunk 数上限：分页模式连续翻页时避免整本书常驻内存；
 * 超出按插入序（FIFO）淘汰最旧块，被淘汰的块在再次访问时重新加载 */
export const MAX_CACHED_CHUNKS = 512;

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
  /** 含非 BMP 字符（码元数 ≠ 码点数）：切片须走码点换算的慢路径 */
  astral: boolean;
  /** server-reported char offset right after this chunk */
  next: number;
  /** 码点数：与服务端 chars().count() 同口径，切片上界按它钳制 */
  scalarLength: number;
  text: string;
}

/**
 * 按 Unicode 标量（码点）计数与切片，对齐服务端 `chars()` 口径。
 * JS 字符串下标是 UTF-16 码元：含非 BMP 字符（emoji、CJK 扩展汉字）时
 * 码元数 > 码点数，直接用原生 length/slice 会让段落偏移、章节跳转与
 * 服务端保存的进度随字符数线性漂移。
 */

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

  /** Sync slice; caller must ensureRange first. Tolerates partial coverage.
   *  start/end 为码点口径（与服务端偏移一致） */
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
      const to = Math.min(end - at, chunk.scalarLength);
      out += chunk.astral
        ? sliceByCodePoints(chunk.text, from, to)
        : chunk.text.slice(from, to);
    }
    return out;
  }

  private load(at: number): Promise<void> {
    const existing = this.pending.get(at);
    if (existing) return existing;
    const job = this.loader(at, TXT_CHUNK_SIZE)
      .then(res => {
        const text = res.content;
        const scalarLength = codePointLength(text);
        this.chunks.set(at, {
          astral: scalarLength !== text.length,
          next: res.next_position,
          scalarLength,
          text,
        });
        // FIFO 淘汰：Map 迭代序即插入序，首个 key 为最旧 chunk
        if (this.chunks.size > MAX_CACHED_CHUNKS) {
          const oldest = this.chunks.keys().next().value;
          if (oldest !== undefined && oldest !== at) this.chunks.delete(oldest);
        }
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
export function codePointLength(text: string): number {
  let count = 0;
  for (let at = 0; at < text.length;) {
    at += codePointSizeAt(text, at);
    count++;
  }
  return count;
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

// --- 章节范围换算（TXT 章节由后端按行解析，position 为字符偏移） ---

/** 按码点下标切片 [start, end)，start/end 与服务端字符偏移同单位 */
export function sliceByCodePoints(
  text: string,
  start: number,
  end: number,
): string {
  if (start < 0 || start >= end) return '';
  let at = 0;
  let cp = 0;
  let startAt = -1;
  while (at < text.length && cp < end) {
    if (cp === start) startAt = at;
    at += codePointSizeAt(text, at);
    cp++;
  }
  // start 超出文本码点数时无内容可取
  return startAt === -1 ? '' : text.slice(startAt, at);
}

/** 下标处码点的码元步长：高代理项后跟低代理项为 2，孤立代理按 1 兜底 */
function codePointSizeAt(text: string, at: number): number {
  const code = text.charCodeAt(at);
  if (code >= 0xd800 && code <= 0xdbff) {
    const next = text.charCodeAt(at + 1);
    if (next >= 0xdc00 && next <= 0xdfff) return 2;
  }
  return 1;
}
