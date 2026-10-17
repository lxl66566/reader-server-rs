// 书内搜索命中 → 跳转目标的统一换算（阅读页 navigator 与详情页深链共用）。

import type { BookDetail, BookFormat, BookSearchMatch } from '../api/types';

/** 服务端全文搜索仅覆盖有文本坐标系的格式；pdf 无文本层暂不支持 */
export const bookSupportsSearch = (format: BookFormat): boolean =>
  format === 'txt' || format === 'epub';

export type SearchJumpTarget =
  | { chapterId: number; kind: 'chapter'; position: number }
  | { kind: 'locator'; locator: string };

/**
 * 命中 → 跳转目标：txt 用字符偏移 locator 精确定位；epub 无 char 坐标系，
 * 按 chapters[chapter_index] 落到章节（position = spine 索引）。索引越界
 * （书籍重传后章节表变化等）返回 null 由调用方忽略
 */
export const searchJumpTarget = (
  book: BookDetail,
  match: BookSearchMatch,
): null | SearchJumpTarget => {
  if (book.format === 'epub') {
    const chapter = book.chapters[match.chapter_index];
    return chapter
      ? {
          chapterId: chapter.chapter_id,
          kind: 'chapter',
          position: chapter.position,
        }
      : null;
  }
  return { kind: 'locator', locator: String(match.char_offset) };
};
