// TXT 划词高亮纯逻辑：locator 编解码、批注→窗口裁剪、段落拆分与分段。
// 所有 offset 均为全书字符偏移（Unicode 码点口径，与服务端 chars() 一致）。

import type { Annotation } from '../api/types';
import { stripParagraphIndent } from './paragraph';
import { codePointLength } from './textSource';

/** 字符区间 [start, end)，码点口径 */
export interface CharRange {
  end: number;
  start: number;
}

/** 高亮 locator 载荷：章节下标 + 字符区间 */
export interface HighlightLocator extends CharRange {
  chapter: number;
}

/** 可渲染到正文的高亮：批注 id + 裁剪后的字符区间 */
export interface HighlightRange extends CharRange {
  id: number;
}

/** 拆分出的段落：text 已剥离段首缩进，indent 记录剥离长度以便还原原文偏移 */
export interface SplitParagraph {
  indent: number;
  offset: number;
  text: string;
}

/** 段落内的连续分段；高亮分段带所属批注 id */
export interface TextSegment {
  end: number;
  highlighted: boolean;
  id?: number;
  start: number;
}

/**
 * DOM 选区边界 → 段内码点偏移（Range offset 为 UTF-16 码元口径，需换算）；
 * node 不是 p 的文本后代时返回 null。
 */
export function charOffsetInParagraph(
  p: HTMLElement,
  node: Node,
  unitOffset: number,
): null | number {
  const walker = document.createTreeWalker(p, NodeFilter.SHOW_TEXT);
  let cp = 0;
  for (let cur = walker.nextNode(); cur; cur = walker.nextNode()) {
    const text = cur.textContent ?? '';
    if (cur === node) {
      return cp + codePointLength(text.slice(0, unitOffset));
    }
    cp += codePointLength(text);
  }
  return null;
}

/** 高亮 locator：`h{chapter}:{start}-{end}`，数十字节，远小于 4KB 上限 */
export function encodeHighlightLocator(locator: HighlightLocator): string {
  return `h${locator.chapter}:${locator.start}-${locator.end}`;
}

/**
 * 批注对应的字符区间；不可渲染（note / locator 非法 / 空区间）返回 null。
 * start_locator 编码完整区间，end_locator 为区间终点的纯数字偏移，
 * 两者均可独立解析，end_locator 缺失时退化为区间编码值。
 */
export function highlightRangeOf(
  annotation: Annotation,
): HighlightRange | null {
  if (annotation.kind !== 'highlight') return null;
  const start = parseHighlightLocator(annotation.start_locator);
  if (!start) return null;
  const endOffset = Number.parseInt(annotation.end_locator ?? '', 10);
  const end = Number.isFinite(endOffset) ? endOffset : start.end;
  if (end <= start.start) return null;
  return { end, id: annotation.id, start: start.start };
}

/** 与窗口 [winStart, winEnd) 相交的高亮，裁剪到窗口内并按起点排序 */
export function highlightsInWindow(
  annotations: readonly Annotation[],
  winStart: number,
  winEnd: number,
): HighlightRange[] {
  const out: HighlightRange[] = [];
  for (const annotation of annotations) {
    const range = highlightRangeOf(annotation);
    if (!range) continue;
    const start = Math.max(range.start, winStart);
    const end = Math.min(range.end, winEnd);
    if (end > start) out.push({ end, id: range.id, start });
  }
  return out.toSorted((a, b) => a.start - b.start || a.end - b.end);
}

/** 选区所在的正文段落元素（带 data-pos），不在正文内时返回 null */
export function paragraphElementOf(node: Node): HTMLElement | null {
  const el = node instanceof HTMLElement ? node : node.parentElement;
  return el?.closest<HTMLElement>('p[data-pos]') ?? null;
}

/** 解析高亮 locator；进度类纯数字 locator 与非法串返回 null */
export function parseHighlightLocator(
  locator: string,
): HighlightLocator | null {
  const matched = /^h(\d+):(\d+)-(\d+)$/.exec(locator);
  if (!matched) return null;
  const chapter = Number(matched[1]);
  const start = Number(matched[2]);
  const end = Number(matched[3]);
  if (![chapter, end, start].every(Number.isFinite) || end <= start) {
    return null;
  }
  return { chapter, end, start };
}

/**
 * 把段落窗口 [paraStart, paraEnd) 按高亮切成有序分段：相邻同态合并，
 * 重叠区间归并给先出现的高亮（渲染上不叠加）；窗口外的高亮忽略。
 */
export function segmentParagraph(
  paraStart: number,
  paraEnd: number,
  highlights: readonly HighlightRange[],
): TextSegment[] {
  if (paraEnd <= paraStart) return [];
  const bounds = new Set<number>([paraEnd, paraStart]);
  for (const range of highlights) {
    if (range.start > paraStart && range.start < paraEnd)
      bounds.add(range.start);
    if (range.end > paraStart && range.end < paraEnd) bounds.add(range.end);
  }
  const sorted = [...bounds].toSorted((a, b) => a - b);
  const out: TextSegment[] = [];
  for (const [at, next] of pairs(sorted)) {
    const hit = highlights.find(h => h.start <= at && h.end >= next);
    const last = out.at(-1);
    if (
      last &&
      last.highlighted === Boolean(hit) &&
      last.id === hit?.id &&
      last.end === at
    ) {
      last.end = next; // 相邻同态合并
    } else {
      out.push({
        end: next,
        highlighted: hit !== undefined,
        ...(hit ? { id: hit.id } : {}),
        start: at,
      });
    }
  }
  return out;
}

/** 按 '\n' 把文本窗口拆成带原文偏移的段落（offset 为含缩进的段首偏移） */
export function splitParagraphs(
  text: string,
  startOffset: number,
): SplitParagraph[] {
  const out: SplitParagraph[] = [];
  let at = startOffset;
  for (const piece of text.split('\n')) {
    const stripped = stripParagraphIndent(piece);
    out.push({
      indent: codePointLength(piece) - codePointLength(stripped),
      offset: at,
      text: stripped,
    });
    at += codePointLength(piece) + 1; // +1 for the newline
  }
  return out;
}

/** 相邻边界对（[v0,v1), [v1,v2), …）；配合 noUncheckedIndexedAccess 的安全迭代 */
function pairs(sorted: number[]): [number, number][] {
  const out: [number, number][] = [];
  for (let i = 0; i + 1 < sorted.length; i++) {
    const at = sorted[i];
    const next = sorted[i + 1];
    if (at !== undefined && next !== undefined) out.push([at, next]);
  }
  return out;
}
