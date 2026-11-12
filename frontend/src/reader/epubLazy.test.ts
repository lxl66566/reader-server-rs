import { describe, expect, it } from 'vitest';

import { extOf } from './epubLazy';

// 条目 URL 的 token 是 JWT（自身含点号），扩展名必须取最后一段
const BASE = '/api/v1/books/1/epub/eyJhbGci.eyJzdWIi9.SflKx';

describe('extOf', () => {
  it('takes extension of the last path segment', () => {
    expect(extOf(`${BASE}/OEBPS/c1.xhtml`)).toBe('xhtml');
    expect(extOf(`${BASE}/META-INF/container.xml`)).toBe('xml');
  });

  it('strips query and hash before matching', () => {
    expect(extOf(`${BASE}/OEBPS/c1.xhtml?x=1#frag`)).toBe('xhtml');
  });

  it('lowercases and returns empty when no extension', () => {
    expect(extOf(`${BASE}/OEBPS/c1.XHTML`)).toBe('xhtml');
    expect(extOf(`${BASE}/mimetype`)).toBe('');
    expect(extOf(`${BASE}/`)).toBe('');
  });
});
