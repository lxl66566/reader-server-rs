import { describe, expect, it } from 'vitest';

import { keyActionOf } from './keyboard';

const none = { altKey: false, ctrlKey: false, metaKey: false, shiftKey: false };

describe('keyActionOf', () => {
  it('maps space / enter / page down / arrow down to page next', () => {
    expect(keyActionOf(' ', none)).toBe('pageNext');
    expect(keyActionOf('Enter', none)).toBe('pageNext');
    expect(keyActionOf('PageDown', none)).toBe('pageNext');
    expect(keyActionOf('ArrowDown', none)).toBe('pageNext');
  });

  it('maps page up / arrow up / shift+space to page prev', () => {
    expect(keyActionOf('PageUp', none)).toBe('pagePrev');
    expect(keyActionOf('ArrowUp', none)).toBe('pagePrev');
    expect(keyActionOf(' ', { ...none, shiftKey: true })).toBe('pagePrev');
  });

  it('maps horizontal arrows to chapter switching', () => {
    expect(keyActionOf('ArrowLeft', none)).toBe('chapterPrev');
    expect(keyActionOf('ArrowRight', none)).toBe('chapterNext');
  });

  it('leaves modifier combos to browser shortcuts', () => {
    expect(keyActionOf(' ', { ...none, ctrlKey: true })).toBe(null);
    expect(keyActionOf('ArrowRight', { ...none, metaKey: true })).toBe(null);
    expect(keyActionOf('PageUp', { ...none, altKey: true })).toBe(null);
  });

  it('ignores unrelated keys', () => {
    expect(keyActionOf('Home', none)).toBe(null);
    expect(keyActionOf('Escape', none)).toBe(null);
    expect(keyActionOf('a', none)).toBe(null);
  });
});
