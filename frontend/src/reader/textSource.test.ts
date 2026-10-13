import { describe, expect, it } from 'vitest';

import {
  chapterBounds,
  chapterIndexAt,
  codePointLength,
  extendWindowDown,
  extendWindowUp,
  jumpWindow,
  MAX_SCROLL_WINDOW_CHUNKS,
  sliceByCodePoints,
  TextSource,
  TXT_CHUNK_SIZE,
} from './textSource';

// fake server: chars are the decimal digits of their position, mod 10
const TOTAL = TXT_CHUNK_SIZE * 2 + 1234;

function expected(start: number, end: number): string {
  let out = '';
  for (let i = start; i < Math.min(end, TOTAL); i++) out += String(i % 10);
  return out;
}

function fakeLoader(
  position: number,
  length: number,
): Promise<{ content: string; next_position: number }> {
  const end = Math.min(position + length, TOTAL);
  let content = '';
  for (let i = position; i < end; i++) content += String(i % 10);
  return Promise.resolve({ content, next_position: end });
}

describe('TextSource', () => {
  it('slices within a single chunk', async () => {
    const source = new TextSource(1, TOTAL, fakeLoader);
    await source.ensureRange(100, 200);
    expect(source.slice(100, 200)).toBe(expected(100, 200));
  });

  it('slices across chunk boundaries', async () => {
    const source = new TextSource(1, TOTAL, fakeLoader);
    const start = TXT_CHUNK_SIZE - 500;
    const end = TXT_CHUNK_SIZE + 500;
    await source.ensureRange(start, end);
    expect(source.slice(start, end)).toBe(expected(start, end));
  });

  it('clamps slicing at the end of the book', async () => {
    const source = new TextSource(1, TOTAL, fakeLoader);
    await source.ensureRange(TOTAL - 10, TOTAL + 100);
    expect(source.slice(TOTAL - 10, TOTAL + 100)).toBe(
      expected(TOTAL - 10, TOTAL),
    );
  });

  it('dedupes concurrent loads of the same chunk', async () => {
    let calls = 0;
    const countingLoader = (position: number, length: number) => {
      calls += 1;
      return fakeLoader(position, length);
    };
    const source = new TextSource(1, TOTAL, countingLoader);
    await Promise.all([
      source.ensureRange(0, 100),
      source.ensureRange(50, 150),
    ]);
    expect(calls).toBe(1);
  });

  it('returns a partial slice when later chunks are missing', async () => {
    const source = new TextSource(1, TOTAL, fakeLoader);
    await source.ensureRange(0, 100);
    // second chunk never loaded: slice stops at the chunk boundary
    expect(source.slice(TXT_CHUNK_SIZE - 10, TXT_CHUNK_SIZE + 10)).toBe(
      expected(TXT_CHUNK_SIZE - 10, TXT_CHUNK_SIZE),
    );
  });
  it('slices by code points when chunks contain non-BMP chars', async () => {
    // 非 BMP 字符（𝄞 U+1D11E、𠀀 U+20000）占 2 个码元但按 1 个码点计，
    // 服务端 total_chars / locator / 章节偏移均为码点口径
    const book = `𝄞${'a'.repeat(9)}\n${'𠀀'.repeat(5)}b\n${'c'.repeat(TXT_CHUNK_SIZE)}tail`;
    const total = codePointLength(book);
    const loader = (position: number, length: number) => {
      const end = Math.min(position + length, total);
      return Promise.resolve({
        content: sliceByCodePoints(book, position, end),
        next_position: end,
      });
    };
    const source = new TextSource(1, total, loader);

    // 跨 chunk、跨代理对切片：结果与按码点构造的期望一致
    await source.ensureRange(TXT_CHUNK_SIZE - 3, TXT_CHUNK_SIZE + 3);
    expect(source.slice(TXT_CHUNK_SIZE - 3, TXT_CHUNK_SIZE + 3)).toBe(
      sliceByCodePoints(book, TXT_CHUNK_SIZE - 3, TXT_CHUNK_SIZE + 3),
    );
    // 全书往返：任意码点区间切片不劈开代理对
    for (const [from, to] of [
      [0, 1],
      [1, 8],
      [total - 4, total],
    ] as const) {
      expect(source.slice(from, to)).toBe(sliceByCodePoints(book, from, to));
      expect(/[\ud800-\udbff]$/.exec(source.slice(from, to))).toBeNull();
    }
  });
});

describe('code point helpers', () => {
  it('counts surrogate pairs as one code point', () => {
    expect(codePointLength('abc')).toBe(3);
    expect(codePointLength('𝄞')).toBe(1);
    expect(codePointLength('𠀀𠀀x')).toBe(3);
    // 孤立高代理按 1 计（防御路径，正常内容不出现）
    expect(codePointLength('\ud800x')).toBe(2);
  });

  it('slices on code point boundaries', () => {
    const text = 'a𝄞b𠀀c';
    expect(sliceByCodePoints(text, 0, 2)).toBe('a𝄞');
    expect(sliceByCodePoints(text, 2, 4)).toBe('b𠀀');
    expect(sliceByCodePoints(text, 4, 5)).toBe('c');
    // 越界钳制
    expect(sliceByCodePoints(text, 4, 99)).toBe('c');
    expect(sliceByCodePoints(text, 99, 100)).toBe('');
    expect(sliceByCodePoints(text, 2, 2)).toBe('');
  });
});

describe('scroll window', () => {
  const TOTAL_CHUNKS = 30;

  it('extends down freely while under the cap', () => {
    const win = { end: 3, start: 0 };
    expect(extendWindowDown(win, TOTAL_CHUNKS, true)).toEqual({
      end: 4,
      start: 0,
    });
  });

  it('drops the top chunk when growing past the cap', () => {
    const win = { end: MAX_SCROLL_WINDOW_CHUNKS, start: 0 };
    expect(extendWindowDown(win, TOTAL_CHUNKS, true)).toEqual({
      end: MAX_SCROLL_WINDOW_CHUNKS + 1,
      start: 1,
    });
  });

  it('keeps growing past the cap when the top cannot be dropped', () => {
    const win = { end: MAX_SCROLL_WINDOW_CHUNKS, start: 0 };
    expect(extendWindowDown(win, TOTAL_CHUNKS, false)).toEqual({
      end: MAX_SCROLL_WINDOW_CHUNKS + 1,
      start: 0,
    });
  });

  it('clamps at the last chunk of the book', () => {
    const win = { end: TOTAL_CHUNKS, start: TOTAL_CHUNKS - 3 };
    expect(extendWindowDown(win, TOTAL_CHUNKS, true)).toEqual({
      end: TOTAL_CHUNKS,
      start: TOTAL_CHUNKS - 3,
    });
  });

  it('extends up freely while under the cap', () => {
    const win = { end: 8, start: 5 };
    expect(extendWindowUp(win, 0, true)).toEqual({ end: 8, start: 4 });
  });

  it('drops the bottom chunk when growing past the cap', () => {
    const win = { end: 20, start: 20 - MAX_SCROLL_WINDOW_CHUNKS };
    expect(extendWindowUp(win, 0, true)).toEqual({
      end: 19,
      start: 20 - MAX_SCROLL_WINDOW_CHUNKS - 1,
    });
  });

  it('keeps growing past the cap when the bottom cannot be dropped', () => {
    const win = { end: 20, start: 20 - MAX_SCROLL_WINDOW_CHUNKS };
    expect(extendWindowUp(win, 0, false)).toEqual({
      end: 20,
      start: 20 - MAX_SCROLL_WINDOW_CHUNKS - 1,
    });
  });

  it('clamps at the first chunk of the book', () => {
    const win = { end: 3, start: 0 };
    expect(extendWindowUp(win, 0, true)).toEqual({ end: 3, start: 0 });
  });

  it('clamps at the chapter start chunk', () => {
    // minStart 不为 0（章节从中部 chunk 开始）时，start 不得越过章节边界
    const win = { end: 8, start: 3 };
    expect(extendWindowUp(win, 3, true)).toEqual({ end: 8, start: 3 });
  });

  it('keeps the window (same reference) when the jump target is inside', () => {
    const win = { end: 8, start: 5 };
    expect(jumpWindow(win, 6, TOTAL_CHUNKS)).toBe(win);
  });

  it('resets the window around a distant jump target', () => {
    const win = { end: 8, start: 5 };
    expect(jumpWindow(win, 25, TOTAL_CHUNKS)).toEqual({ end: 26, start: 25 });
    // 跳到末尾附近时 end 受 totalChunks 约束
    expect(jumpWindow(win, TOTAL_CHUNKS - 1, TOTAL_CHUNKS)).toEqual({
      end: TOTAL_CHUNKS,
      start: TOTAL_CHUNKS - 1,
    });
  });
});

describe('chapter bounds', () => {
  // 章节边界不与 6000 字符的 chunk 对齐
  const chapters = [{ position: 0 }, { position: 7000 }, { position: 15000 }];
  const BOOK_TOTAL = 20000;

  it('returns per-chapter ranges and the whole book for an empty list', () => {
    expect(chapterBounds(chapters, 0, BOOK_TOTAL)).toEqual({
      end: 7000,
      start: 0,
    });
    expect(chapterBounds(chapters, 1, BOOK_TOTAL)).toEqual({
      end: 15000,
      start: 7000,
    });
    // 最后一章延伸到全书末尾
    expect(chapterBounds(chapters, 2, BOOK_TOTAL)).toEqual({
      end: BOOK_TOTAL,
      start: 15000,
    });
    expect(chapterBounds([], 0, BOOK_TOTAL)).toEqual({
      end: BOOK_TOTAL,
      start: 0,
    });
  });

  it('finds the chapter containing an offset', () => {
    expect(chapterIndexAt(chapters, 0)).toBe(0);
    expect(chapterIndexAt(chapters, 6999)).toBe(0);
    // 边界偏移本身归属后一章
    expect(chapterIndexAt(chapters, 7000)).toBe(1);
    expect(chapterIndexAt(chapters, 19999)).toBe(2);
  });
});
