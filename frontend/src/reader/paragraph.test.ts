import { describe, expect, it } from 'vitest';

import { stripParagraphIndent } from './paragraph';

describe('stripParagraphIndent', () => {
  it('strips leading full-width and ascii whitespace', () => {
    expect(stripParagraphIndent('　　正文内容')).toBe('正文内容');
    expect(stripParagraphIndent(' \t\u3000正文')).toBe('正文');
    expect(stripParagraphIndent('正文内容')).toBe('正文内容');
  });

  it('keeps whitespace inside the paragraph', () => {
    expect(stripParagraphIndent('段内　　空格')).toBe('段内　　空格');
    expect(stripParagraphIndent('')).toBe('');
  });
});
