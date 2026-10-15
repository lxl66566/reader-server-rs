// Contract between the reader shell (pages/Reader.tsx) and per-format readers.

import type { BookDetail } from '../api/types';

/** 详情页深链：定位到书签/批注位置，或经后端解析章节后跳转 */
export type DeepLink =
  | { chapterId: number; kind: 'chapter' }
  | { kind: 'locator'; locator: string };

export interface FormatReaderProps {
  /** bump to tell readers to repaint annotations/highlights */
  annotationsVersion: () => number;
  book: BookDetail;
  /** 详情页深链：各 Reader 在自身就绪后消费（仅首次加载有效） */
  deepLink?: DeepLink | null;
  /** progress locator saved on the server (null = never read) */
  initialLocator: null | string;
  /**
   * 用户交互信号（含 epub iframe 内部——其事件不会冒泡到父窗口），
   * 供心跳的活跃检测使用
   */
  onActivity?: () => void;
  /** 批注在本端写入（创建/删除/编辑）后回调：shell 刷新列表并广播重绘信号 */
  onAnnotationsChanged?: () => void;
  /** report position changes for heartbeat and the chrome percent display */
  onPosition: (locator: string, percent: number) => void;
  /** tap on the middle zone of the page: shell toggles the chrome bars */
  onTapCenter: () => void;
  /** register navigation capabilities once the reader is ready */
  registerNavigator: (nav: ReaderNavigator) => void;
}

export interface ReaderNavigator {
  /** 跳转到章节位置（txt = 字符偏移，epub = spine 下标）；false = 无该章或失败 */
  jumpToChapter: (position: number) => Promise<boolean>;
  /** 跳转到任意 locator（书签/批注/跨设备同步）；false = locator 无效或跳转失败 */
  jumpToLocator: (locator: string) => Promise<boolean>;
}
