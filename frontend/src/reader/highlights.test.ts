import { describe, expect, it } from 'vitest';

import type { Annotation } from '../api/types';
import {
  encodeHighlightLocator,
  highlightRangeOf,
  highlightsInWindow,
  parseHighlightLocator,
  segmentParagraph,
  splitParagraphs,
} from './highlights';

// Annotation 字面量按接口成员序展开，测试里只关心 id / kind / locator 三字段
const annotationOf = (
  id: number,
  startLocator: string,
  overrides: Partial<Annotation> = {},
): Annotation => ({
  book_id: 1,
  content: null,
  created_at: '',
  end_locator: null,
  end_percent: null,
  excerpt: null,
  id,
  kind: 'highlight',
  start_locator: startLocator,
  start_percent: 0,
  updated_at: '',
  user_id: 1,
  ...overrides,
});

const highlight = (id: number, start: number, end: number): Annotation =>
  annotationOf(id, encodeHighlightLocator({ chapter: 0, end, start }), {
    end_locator: String(end),
    end_percent: 1,
    start_percent: 0,
  });

// segmentParagraph 直接收裁剪后的区间
const range = (id: number, start: number, end: number) => ({ end, id, start });

describe('highlight locator codec', () => {
  it('round-trips chapter and code point range', () => {
    expect(encodeHighlightLocator({ chapter: 3, end: 456, start: 12 })).toBe(
      'h3:12-456',
    );
    expect(parseHighlightLocator('h3:12-456')).toEqual({
      chapter: 3,
      end: 456,
      start: 12,
    });
  });

  it('rejects progress locators and malformed payloads', () => {
    expect(parseHighlightLocator('12345')).toBeNull(); // 进度 locator
    expect(parseHighlightLocator('')).toBeNull();
    expect(parseHighlightLocator('h0:5-5')).toBeNull(); // 空选区
    expect(parseHighlightLocator('h0:9-5')).toBeNull(); // 逆序区间
    expect(parseHighlightLocator('hx:1-2')).toBeNull();
    expect(parseHighlightLocator('h1:2')).toBeNull();
    expect(parseHighlightLocator('h1:2-3-4')).toBeNull();
  });
});

describe('highlightRangeOf', () => {
  it('prefers the plain-int end_locator over the encoded range', () => {
    const annotation = annotationOf(
      7,
      encodeHighlightLocator({ chapter: 1, end: 99, start: 10 }),
      { end_locator: '55' },
    );
    expect(highlightRangeOf(annotation)).toEqual({
      end: 55,
      id: 7,
      start: 10,
    });
  });

  it('falls back to the encoded end without end_locator', () => {
    const annotation = annotationOf(
      7,
      encodeHighlightLocator({ chapter: 1, end: 99, start: 10 }),
    );
    expect(highlightRangeOf(annotation)).toEqual({
      end: 99,
      id: 7,
      start: 10,
    });
  });

  it('rejects notes and degenerate ranges', () => {
    expect(
      highlightRangeOf(annotationOf(1, 'h0:1-2', { kind: 'note' })),
    ).toBeNull();
    expect(highlightRangeOf(annotationOf(1, 'h0:5-5'))).toBeNull();
    expect(highlightRangeOf(annotationOf(1, 'h0:9-1'))).toBeNull();
    expect(highlightRangeOf(annotationOf(1, '123'))).toBeNull();
  });
});

describe('highlightsInWindow', () => {
  const annotations = [
    highlight(1, 0, 20),
    highlight(2, 18, 30), // 与窗口部分相交
    highlight(3, 40, 50), // 窗口外
    annotationOf(4, 'h2:5-8', { kind: 'note' }), // 笔记不渲染
  ];

  it('clips intersecting ranges into the window and drops the rest', () => {
    expect(highlightsInWindow(annotations, 10, 25)).toEqual([
      { end: 20, id: 1, start: 10 },
      { end: 25, id: 2, start: 18 },
    ]);
  });

  it('renders a cross-chapter highlight in both chapters, clipped', () => {
    // 跨章高亮：章节 A = [0, 30)，章节 B = [30, 60)
    const cross = [highlight(9, 20, 40)];
    expect(highlightsInWindow(cross, 0, 30)).toEqual([
      { end: 30, id: 9, start: 20 },
    ]);
    expect(highlightsInWindow(cross, 30, 60)).toEqual([
      { end: 40, id: 9, start: 30 },
    ]);
  });

  it('returns empty for empty selection ranges and empty lists', () => {
    expect(highlightsInWindow([], 0, 100)).toEqual([]);
    expect(highlightsInWindow([highlight(1, 5, 5)], 0, 100)).toEqual([]);
  });
});

describe('segmentParagraph', () => {
  it('returns no segments for an empty paragraph', () => {
    expect(segmentParagraph(10, 10, [range(1, 10, 20)])).toEqual([]);
  });

  it('keeps a plain paragraph intact without highlights', () => {
    expect(segmentParagraph(0, 10, [])).toEqual([
      { end: 10, highlighted: false, start: 0 },
    ]);
  });

  it('splits a paragraph around a highlight', () => {
    expect(segmentParagraph(0, 10, [range(1, 3, 5)])).toEqual([
      { end: 3, highlighted: false, start: 0 },
      { end: 5, highlighted: true, id: 1, start: 3 },
      { end: 10, highlighted: false, start: 5 },
    ]);
  });

  it('clips highlights partially outside the paragraph window', () => {
    expect(segmentParagraph(5, 10, [range(1, 3, 7)])).toEqual([
      { end: 7, highlighted: true, id: 1, start: 5 },
      { end: 10, highlighted: false, start: 7 },
    ]);
    expect(segmentParagraph(5, 10, [range(1, 0, 99)])).toEqual([
      { end: 10, highlighted: true, id: 1, start: 5 },
    ]);
  });

  it('merges adjacent segments of the same annotation', () => {
    const touching = [range(1, 2, 4), range(1, 4, 6)];
    expect(segmentParagraph(0, 10, touching)).toEqual([
      { end: 2, highlighted: false, start: 0 },
      { end: 6, highlighted: true, id: 1, start: 2 },
      { end: 10, highlighted: false, start: 6 },
    ]);
  });

  it('resolves overlaps to the first highlight', () => {
    const overlapping = [range(1, 2, 8), range(2, 4, 6)];
    expect(segmentParagraph(0, 10, overlapping)).toEqual([
      { end: 2, highlighted: false, start: 0 },
      { end: 8, highlighted: true, id: 1, start: 2 },
      { end: 10, highlighted: false, start: 8 },
    ]);
  });
});

describe('splitParagraphs', () => {
  it('tracks code point offsets across newlines and indents', () => {
    const paras = splitParagraphs('　　首段\n次段', 100);
    expect(paras).toEqual([
      { indent: 2, offset: 100, text: '首段' },
      { indent: 0, offset: 105, text: '次段' },
    ]);
  });

  it('counts astral chars as single code points', () => {
    const paras = splitParagraphs('𠮷\nab', 0);
    expect(paras).toEqual([
      { indent: 0, offset: 0, text: '𠮷' },
      { indent: 0, offset: 2, text: 'ab' },
    ]);
  });
});
