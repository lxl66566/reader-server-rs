import { describe, expect, it } from 'vitest';

import { TextSource, TXT_CHUNK_SIZE } from './textSource';

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
