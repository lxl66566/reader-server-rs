import { describe, expect, it } from 'vitest';

import { TXT_CHUNK_SIZE } from '../reader/textSource';
import {
  chapterSpans,
  chunksForRange,
  coverageOf,
  rangeFromChapter,
} from './coverage';

const chapters = [
  { position: 0 },
  { position: 7000 },
  { position: 15000 },
  { position: 20000 },
];

describe('chunksForRange', () => {
  it('aligns to chunk boundaries and clamps at totalChars', () => {
    expect(chunksForRange(0, 1, 999_999)).toEqual([0]);
    // start 6100 -> chunk start 6000；end 13000 覆盖 6000/12000 两块
    expect(chunksForRange(6100, 13_000, 999_999)).toEqual([6000, 12_000]);
    // 尾部钳制：totalChars 落在块中间时最后一块只到 totalChars 为止
    expect(chunksForRange(12_000, 14_000, 13_000)).toEqual([12_000]);
  });
});

describe('coverageOf', () => {
  it('marks chapters covered only when every chunk is cached', () => {
    const spans = chapterSpans(chapters, 23_000);
    // 第 0 章 [0,7000)：chunks 0,6000；第 1 章 [7000,15000)：6000,12000；
    // 第 2 章 [15000,20000)：12000,18000；第 3 章 [20000,23000)：18000
    const covered = coverageOf(spans, [0, 6000, 18_000]).covered;
    expect(covered).toEqual([0, 3]);
  });

  it('treats missing middle chunk as uncovered', () => {
    const spans = chapterSpans(chapters, 23_000);
    expect(coverageOf(spans, [0, 6000, 12_000, 18_000]).covered).toEqual([
      0, 1, 2, 3,
    ]);
    expect(coverageOf(spans, [0, 12_000, 18_000]).missing).toContain(0);
  });

  it('handles empty chapter list', () => {
    expect(chapterSpans([], 100)).toEqual([]);
    expect(coverageOf([], [0, 6000])).toEqual({ covered: [], missing: [] });
  });
});

describe('rangeFromChapter', () => {
  it('returns the char range spanning count chapters', () => {
    expect(rangeFromChapter(chapters, 1, 2, 23_000)).toEqual({
      end: 20_000,
      start: 7000,
    });
  });

  it('clamps to totalChars at the book end', () => {
    expect(rangeFromChapter(chapters, 3, 50, 23_000)).toEqual({
      end: 23_000,
      start: 20_000,
    });
  });

  it('returns null for an out-of-range start', () => {
    expect(rangeFromChapter(chapters, 4, 1, 23_000)).toBeNull();
  });
});

describe('chapter spans align with chunk size', () => {
  it('keeps spans within the book', () => {
    for (const span of chapterSpans(chapters, 23_000)) {
      expect(span.start).toBeLessThan(span.end);
      expect(span.end).toBeLessThanOrEqual(23_000);
      for (const at of span.chunkStarts) {
        expect(at % TXT_CHUNK_SIZE).toBe(0);
        expect(at).toBeLessThan(23_000);
      }
    }
  });
});
