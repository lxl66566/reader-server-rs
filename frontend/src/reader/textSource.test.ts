import { describe, expect, it } from 'vitest';

import {
  extendWindowDown,
  extendWindowUp,
  jumpWindow,
  MAX_SCROLL_WINDOW_CHUNKS,
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
    expect(extendWindowUp(win, true)).toEqual({ end: 8, start: 4 });
  });

  it('drops the bottom chunk when growing past the cap', () => {
    const win = { end: 20, start: 20 - MAX_SCROLL_WINDOW_CHUNKS };
    expect(extendWindowUp(win, true)).toEqual({
      end: 19,
      start: 20 - MAX_SCROLL_WINDOW_CHUNKS - 1,
    });
  });

  it('keeps growing past the cap when the bottom cannot be dropped', () => {
    const win = { end: 20, start: 20 - MAX_SCROLL_WINDOW_CHUNKS };
    expect(extendWindowUp(win, false)).toEqual({
      end: 20,
      start: 20 - MAX_SCROLL_WINDOW_CHUNKS - 1,
    });
  });

  it('clamps at the first chunk of the book', () => {
    const win = { end: 3, start: 0 };
    expect(extendWindowUp(win, true)).toEqual({ end: 3, start: 0 });
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
