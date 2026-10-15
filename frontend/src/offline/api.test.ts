// bookListCacheKey 回归：排序/筛选不同的列表视图不得互相命中脏缓存。
import { describe, expect, it } from 'vitest';

import { bookListCacheKey } from './api';

describe('bookListCacheKey', () => {
  it('同参数生成稳定键', () => {
    const query = { format: 'epub', order: 'asc', sort: 'title' } as const;
    expect(bookListCacheKey(2, 20, query)).toBe(bookListCacheKey(2, 20, query));
  });

  it('排序/方向/格式/分页任一不同即不同键', () => {
    const base = {
      format: 'epub',
      order: 'desc',
      sort: 'title',
    } as const;
    const keys = new Set([
      bookListCacheKey(1, 20),
      bookListCacheKey(1, 20, base),
      bookListCacheKey(1, 20, { ...base, format: 'pdf' }),
      bookListCacheKey(1, 20, { ...base, order: 'asc' }),
      bookListCacheKey(1, 20, { ...base, sort: 'author' }),
      bookListCacheKey(2, 20, base),
    ]);
    expect(keys.size).toBe(6);
  });
});
