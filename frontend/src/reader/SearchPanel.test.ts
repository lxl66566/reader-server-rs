import { describe, expect, it } from 'vitest';

import { splitExcerpt } from './searchExcerpt';

describe('splitExcerpt', () => {
  it('bolds the hit at the computed offset', () => {
    // excerpt = 命中前 30 字符 + 命中 + 命中后文：命中起点 = char_offset - 30
    const before = '前'.repeat(30);
    const excerpt = `${before}命中后文`;
    const parts = splitExcerpt(excerpt, 30, 2);
    expect(parts.before).toBe(before);
    expect(parts.hit).toBe('命中');
    expect(parts.after).toBe('后文');
  });

  it('shifts the hit offset when the excerpt start was clamped', () => {
    // 命中在第 3 字符处（前文不足 30），命中在 excerpt 内的偏移 = char_offset 本身
    const parts = splitExcerpt('一二三命中四', 3, 2);
    expect(parts.before).toBe('一二三');
    expect(parts.hit).toBe('命中');
    expect(parts.after).toBe('四');
  });

  it('slices by code points so surrogate pairs stay intact', () => {
    // 🌍 为代理对：按码点切分时不会被切开
    const excerpt = '🌍🌍🌍命中';
    const parts = splitExcerpt(excerpt, 3, 2);
    expect(parts.before).toBe('🌍🌍🌍');
    expect(parts.hit).toBe('命中');
    expect(parts.after).toBe('');
  });
});
