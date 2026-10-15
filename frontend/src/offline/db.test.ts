// clearBookContent 的隔离性回归：bookId 呈前缀关系（1 与 12）时不得误删。
import 'fake-indexeddb/auto';
import { beforeAll, describe, expect, it, vi } from 'vitest';

import type { BookDetail } from '../api/types';
import {
  chunkGet,
  chunkPut,
  clearBookContent,
  fileBytes,
  fileGet,
  filePut,
  metaGet,
  metaPut,
} from './db';

// db.ts 经 api/client 读 localStorage 取用户前缀；node 环境无该全局，
// 桩定为无 token（前缀恒为 anon，测试内确定即可）
beforeAll(() => {
  vi.stubGlobal('localStorage', {
    getItem: () => null,
    removeItem: () => undefined,
    setItem: () => undefined,
  });
});

const detail = (bookId: number): BookDetail => ({
  author: null,
  book_id: bookId,
  chapter_pattern: null,
  chapters: [],
  created_at: '2026-01-01T00:00:00Z',
  description: null,
  format: 'txt',
  is_owner: true,
  is_public: false,
  last_read_at: null,
  locator: null,
  percent: 0,
  reading_time: 0,
  title: `book ${bookId}`,
  total_chars: 10_000,
  total_pages: 0,
});

const seedChunk = (content: string) => ({ content, next_position: 6000 });

describe('clearBookContent', () => {
  it('keeps book 12 intact when clearing book 1 (prefix collision)', async () => {
    await metaPut(1, { detail: detail(1), savedAt: 0 });
    await metaPut(12, { detail: detail(12), savedAt: 0 });
    await chunkPut(1, 0, seedChunk('one'));
    await chunkPut(12, 0, seedChunk('twelve'));
    await filePut(1, new ArrayBuffer(8), 'rev1');
    await filePut(12, new ArrayBuffer(16), 'rev12');

    await clearBookContent(1);

    // 目标书清干净
    expect(await metaGet(1)).toBeNull();
    expect(await chunkGet(1, 0)).toBeNull();
    expect(await fileGet(1)).toBeNull();
    expect(await fileBytes(1)).toBeNull();
    // book 12 的全部缓存完好
    expect((await metaGet(12))?.detail.book_id).toBe(12);
    expect((await chunkGet(12, 0))?.content).toBe('twelve');
    expect((await fileGet(12))?.byteLength).toBe(16);
    expect(await fileBytes(12)).toBe(16);
  });

  it('clears every cached chunk of the target book', async () => {
    await chunkPut(3, 0, seedChunk('a'));
    await chunkPut(3, 6000, seedChunk('b'));
    await chunkPut(30, 0, seedChunk('c'));

    await clearBookContent(3);

    expect(await chunkGet(3, 0)).toBeNull();
    expect(await chunkGet(3, 6000)).toBeNull();
    // id 30 与 3 同样呈前缀关系
    expect((await chunkGet(30, 0))?.content).toBe('c');
  });
});
