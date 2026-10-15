// txt 章节与 chunk 覆盖关系的纯换算：缓存面板展示与下载范围共用。

import { TXT_CHUNK_SIZE } from '../reader/textSource';

export interface ChapterSpan {
  /** 覆盖该章所需的全部 chunk 起始偏移（升序） */
  chunkStarts: number[];
  /** 章节字符范围 [start, end) */
  end: number;
  start: number;
}

export interface CoverageSummary {
  /** 已完整覆盖的章节下标 */
  covered: number[];
  /** 尚有缺口的章节下标 */
  missing: number[];
}

/** 章节是否被已缓存 chunk 集合完整覆盖 */
export function chapterCovered(
  span: ChapterSpan,
  cached: Set<number>,
): boolean {
  return span.chunkStarts.every(position => cached.has(position));
}

/** 各章节的覆盖需求与字符范围 */
export function chapterSpans(
  chapters: readonly { position: number }[],
  totalChars: number,
): ChapterSpan[] {
  return chapters.map((chapter, index) => {
    const start = chapter.position;
    const end = chapters.at(index + 1)?.position ?? totalChars;
    return { chunkStarts: chunksForRange(start, end, totalChars), end, start };
  });
}

/**
 * 覆盖章节字符范围 [start, end) 所需的 chunk 起始偏移集合。
 * 与 TextSource.ensureRange 同口径：chunk 与 TXT_CHUNK_SIZE 对齐，
 * 上界以 totalChars 钳制（最后一块不足整块）。
 */
export function chunksForRange(
  start: number,
  end: number,
  totalChars: number,
): number[] {
  const out: number[] = [];
  const first =
    Math.floor(Math.max(0, start) / TXT_CHUNK_SIZE) * TXT_CHUNK_SIZE;
  for (let at = first; at < end && at < totalChars; at += TXT_CHUNK_SIZE) {
    out.push(at);
  }
  return out;
}

export function coverageOf(
  spans: readonly ChapterSpan[],
  cachedChunkStarts: readonly number[],
): CoverageSummary {
  const cached = new Set(cachedChunkStarts);
  const covered: number[] = [];
  const missing: number[] = [];
  for (const [index, span] of spans.entries()) {
    if (chapterCovered(span, cached)) covered.push(index);
    else missing.push(index);
  }
  return { covered, missing };
}

/**
 * 自某章起向后取 count 个章节的字符范围（不足时到书尾）。
 * 返回 null 表示起点章节不存在。
 */
export function rangeFromChapter(
  chapters: readonly { position: number }[],
  fromIndex: number,
  count: number,
  totalChars: number,
): null | { end: number; start: number } {
  const startChapter = chapters.at(fromIndex);
  if (!startChapter) return null;
  return {
    end: chapters.at(fromIndex + count)?.position ?? totalChars,
    start: startChapter.position,
  };
}
