// 阅读器键盘操作：统一三种格式、两种翻页模式的按键语义。
// 翻页：space / enter / page down / arrow down 向下，page up / arrow up /
// shift+space 向上（分页模式为一页，滚动模式为 90% 视口的章内滚动）；
// 左右方向键切换章节。

import { isTextEntryTarget } from './gestures';

export type ReaderKeyAction =
  | 'chapterNext'
  | 'chapterPrev'
  | 'pageNext'
  | 'pagePrev';

export interface ReaderKeyHandlers {
  /** 切换章节 */
  chapter: (dir: -1 | 1) => void;
  /** 翻页 / 章内滚动 */
  page: (dir: -1 | 1) => void;
}

/** 在 document 上绑定阅读按键，返回解绑函数；消费的按键阻止默认行为 */
export function bindReaderKeys(
  doc: Document,
  handlers: ReaderKeyHandlers,
): () => void {
  const onKeyDown = (event: KeyboardEvent): void => {
    const action = readerKeyAction(event);
    if (action === null) return;
    event.preventDefault();
    if (action === 'chapterNext' || action === 'chapterPrev') {
      handlers.chapter(action === 'chapterNext' ? 1 : -1);
    } else {
      handlers.page(action === 'pageNext' ? 1 : -1);
    }
  };
  doc.addEventListener('keydown', onKeyDown);
  return () => {
    doc.removeEventListener('keydown', onKeyDown);
  };
}

// 按键映射：enter 同 page down；上下方向键在滚动模式为章内滚动、
// 分页模式为翻页；左右方向键切章
const ACTION_BY_KEY: Record<string, ReaderKeyAction> = {
  ArrowDown: 'pageNext',
  ArrowLeft: 'chapterPrev',
  ArrowRight: 'chapterNext',
  ArrowUp: 'pagePrev',
  Enter: 'pageNext',
  PageDown: 'pageNext',
  PageUp: 'pagePrev',
};

/** 按键到动作的纯映射（不含 DOM 目标判定）；null = 不消费 */
export function keyActionOf(
  key: string,
  mods: {
    altKey: boolean;
    ctrlKey: boolean;
    metaKey: boolean;
    shiftKey: boolean;
  },
): null | ReaderKeyAction {
  // 组合键留给浏览器/应用快捷键（如 ctrl+f 搜索）
  if (mods.altKey || mods.ctrlKey || mods.metaKey) return null;
  // shift+space 反向翻页（通用阅读器惯例）
  if (key === ' ') return mods.shiftKey ? 'pagePrev' : 'pageNext';
  return ACTION_BY_KEY[key] ?? null;
}

/** 键盘事件到阅读动作；输入框 / 可激活元素 / 组合键不消费 */
export function readerKeyAction(event: KeyboardEvent): null | ReaderKeyAction {
  if (isTextEntryTarget(event.target) || isInteractiveTarget(event.target)) {
    return null;
  }
  return keyActionOf(event.key, event);
}

// 按钮等可激活元素上的 space/enter 保留原生点击语义（如弹窗按钮的回车确认）
const INTERACTIVE_SELECTOR = 'a, button, select, summary, [role="button"]';

function isInteractiveTarget(target: EventTarget | null): boolean {
  return (
    target instanceof Element && target.closest(INTERACTIVE_SELECTOR) !== null
  );
}
