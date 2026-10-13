// Contract between the reader shell (pages/Reader.tsx) and per-format readers.

import type { BookDetail } from '../api/types';

export interface FormatReaderProps {
  /** bump to tell readers to repaint annotations/highlights */
  annotationsVersion: () => number;
  book: BookDetail;
  /** progress locator saved on the server (null = never read) */
  initialLocator: null | string;
  /** report position changes for heartbeat and the chrome percent display */
  onPosition: (locator: string, percent: number) => void;
  /** tap on the middle zone of the page: shell toggles the chrome bars */
  onTapCenter: () => void;
  /** register navigation capabilities once the reader is ready */
  registerNavigator: (nav: ReaderNavigator) => void;
}

export interface ReaderNavigator {
  /** jump to a chapter position (txt = char offset, epub = spine index) */
  jumpToChapter: (position: number) => void;
  /** jump to an arbitrary locator (bookmark / annotation / cross-device sync) */
  jumpToLocator: (locator: string) => void;
}
