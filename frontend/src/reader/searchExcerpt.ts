// In-book search excerpt splitting (pure helpers, no JSX — unit-testable).

/** 与后端 BOOK_SEARCH_EXCERPT_CONTEXT 对齐：excerpt 在命中前最多保留的字符数 */
export const EXCERPT_CONTEXT_CHARS = 30;

export interface ExcerptSegments {
  after: string;
  before: string;
  hit: string;
}

/**
 * 按命中位置切分 excerpt 用于加粗：命中在 excerpt 内的偏移 =
 * char_offset - excerpt 起点截断值。全部按码点切，避免代理对被切开；
 * 命中段长度取查询词码点数（后端为字面匹配，长度必然一致）
 */
export function splitExcerpt(
  excerpt: string,
  charOffset: number,
  queryChars: number,
): ExcerptSegments {
  const hitStart = charOffset - Math.max(0, charOffset - EXCERPT_CONTEXT_CHARS);
  const chars = Array.from(excerpt);
  const clamped = Math.min(hitStart, chars.length);
  return {
    after: chars.slice(clamped + queryChars).join(''),
    before: chars.slice(0, clamped).join(''),
    hit: chars.slice(clamped, clamped + queryChars).join(''),
  };
}
