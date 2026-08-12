// EPUB reader: epub.js rendition with theme sync, swipe/tap gestures and
// locator = EpubCFI, percent via generated locations.

import ePub, { type Book, type Rendition } from 'epubjs';
import { createEffect, on, onCleanup, onMount, Show } from 'solid-js';
import { bookApi } from '../api';
import { Spinner } from '../components/ui';
import { fontFamilyCss, useSettings } from '../stores/settings';
import { loadChineseConverter, toSimplified } from '../utils/opencc';
import type { FormatReaderProps } from './types';

const TAP_MAX_MS = 250;
const TAP_MAX_MOVE_PX = 10;
const FLIP_THRESHOLD_PX = 60;

const EpubReader = (props: FormatReaderProps) => {
  const { settings } = useSettings();
  let container: HTMLDivElement | undefined;

  let book: Book | null = null;
  let rendition: null | Rendition = null;
  let ready = false;
  let pendingLocator: null | string = null;
  let locationsReady = false;
  let lastCfi: null | string = null;

  onMount(() => {
    void setup();
    onCleanup(() => {
      ready = false;
      book?.destroy();
      book = null;
      rendition = null;
    });
  });

  async function setup(): Promise<void> {
    if (!container) return;
    const data = await bookApi.fetchFile(props.book.book_id);
    book = ePub(data);
    await book.ready;

    const scrolled = settings.pagination_mode === 'scroll';
    rendition = book.renderTo(container, {
      flow: scrolled ? 'scrolled-doc' : 'paginated',
      height: '100%',
      manager: scrolled ? 'continuous' : 'default',
      width: '100%',
    });

    registerTheme();
    registerContentHooks();

    rendition.on('relocated', (location: { start: { cfi: string } }) => {
      lastCfi = location.start.cfi;
      reportPosition();
    });

    // initial position: saved cfi, or percent-based cfi once locations exist
    const startAt = props.initialLocator;
    if (startAt && startAt.includes('epubcfi')) {
      await rendition.display(startAt);
    } else {
      await rendition.display();
    }

    ready = true;
    if (pendingLocator) {
      await displaySafe(pendingLocator);
      pendingLocator = null;
    }

    // locations give us percent mapping; generate in the background
    void book.locations
      .generate(1600)
      .then(() => {
        locationsReady = true;
        // honor a bare percent progress when no cfi was saved
        if (!startAt?.includes('epubcfi') && props.book.percent > 0 && book) {
          const cfi = book.locations.cfiFromPercentage(props.book.percent);
          void displaySafe(cfi);
        }
        reportPosition();
      })
      .catch(() => {
        // locations are best-effort: progress display degrades to 0
      });

    reportPosition();
  }

  async function displaySafe(target: string): Promise<void> {
    if (!rendition) return;
    try {
      await rendition.display(target);
    } catch {
      // stale locator (e.g. from another epub build): stay where we are
    }
  }

  function reportPosition(): void {
    if (!lastCfi) return;
    let percent = props.book.percent;
    if (locationsReady && book) {
      try {
        percent = book.locations.percentageFromCfi(lastCfi);
      } catch {
        // cfi outside generated locations: keep previous percent
      }
    }
    props.onPosition(lastCfi, Math.min(1, Math.max(0, percent)));
  }

  // --- theming: mirror ReadingSettings into the iframe ---
  function registerTheme(): void {
    if (!rendition) return;
    rendition.themes.register('custom', {
      body: {
        background: 'transparent',
        color: `${settings.text_color} !important`,
        'font-family': fontFamilyCss(settings.font_family) || 'inherit',
        padding: '0 16px',
      },
      p: {
        'line-height': `${settings.line_height} !important`,
        'margin-bottom': `${settings.paragraph_spacing}em !important`,
        'text-indent': `${settings.text_indent}em !important`,
      },
    });
    rendition.themes.select('custom');
    rendition.themes.override('font-size', `${settings.font_size}px`, true);
    rendition.themes.override(
      'letter-spacing',
      `${settings.letter_spacing}px`,
      true,
    );
  }

  // re-apply when settings change (color/font/size)
  createEffect(
    on(
      () => [
        settings.text_color,
        settings.font_family,
        settings.font_size,
        settings.line_height,
        settings.letter_spacing,
        settings.paragraph_spacing,
        settings.text_indent,
      ],
      () => {
        if (rendition) registerTheme();
      },
      { defer: true },
    ),
  );

  // --- content hooks: gestures inside the iframe + optional conversion ---
  function registerContentHooks(): void {
    if (!rendition) return;
    rendition.hooks.content.register((contents: { document: Document }) => {
      if (settings.simplified_chinese) {
        void loadChineseConverter().then(() => {
          convertTree(contents.document);
        });
      }
      attachGestures(contents.document);
    });
  }

  // re-render sections when the conversion toggle flips
  createEffect(() => {
    const enabled = settings.simplified_chinese;
    if (!rendition || !ready) return;
    if (enabled) void loadChineseConverter();
    const current = lastCfi;
    rendition.clear();
    rendition.start();
    if (current) void displaySafe(current);
  });

  function convertTree(doc: Document): void {
    const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT);
    let node = walker.nextNode();
    while (node) {
      node.textContent = toSimplified(node.textContent ?? '');
      node = walker.nextNode();
    }
  }

  // --- gestures: swipe flips a page (paginate mode), tap zones flip/toggle ---
  function attachGestures(doc: Document): void {
    let start: null | { t: number; x: number; y: number } = null;
    doc.addEventListener(
      'touchstart',
      event => {
        const t0 = event.touches[0];
        if (t0) start = { t: Date.now(), x: t0.clientX, y: t0.clientY };
      },
      { passive: true },
    );
    doc.addEventListener(
      'touchend',
      event => {
        if (!start) return;
        const { t, x, y } = start;
        start = null;
        const t0 = event.changedTouches[0];
        if (!t0) return;
        const deltaX = t0.clientX - x;
        const deltaY = t0.clientY - y;
        if (settings.pagination_mode === 'paginate') {
          if (
            Math.abs(deltaX) > FLIP_THRESHOLD_PX &&
            Math.abs(deltaX) > Math.abs(deltaY)
          ) {
            void flip(deltaX < 0 ? 1 : -1);
            return;
          }
        }
        if (
          Date.now() - t < TAP_MAX_MS &&
          Math.hypot(deltaX, deltaY) < TAP_MAX_MOVE_PX
        ) {
          handleTap(t0.clientX);
        }
      },
      { passive: true },
    );
    doc.addEventListener('click', event => {
      handleTap(event.clientX);
    });
  }

  function handleTap(clientX: number): void {
    const width = container?.clientWidth ?? 1;
    const ratio = clientX / width;
    if (settings.pagination_mode === 'paginate') {
      if (ratio < 0.33) {
        void flip(-1);
        return;
      }
      if (ratio > 0.67) {
        void flip(1);
        return;
      }
    }
    props.onTapCenter();
  }

  async function flip(dir: -1 | 1): Promise<void> {
    if (!rendition) return;
    if (dir > 0) await rendition.next();
    else await rendition.prev();
  }

  // switching paginate/scroll needs a fresh rendition (flow is a creation option)
  createEffect(
    on(
      () => settings.pagination_mode,
      () => {
        if (!book) return; // initial setup reads the mode directly
        const current = lastCfi;
        book.destroy();
        book = null;
        rendition = null;
        ready = false;
        void setup().then(() => {
          if (current) void displaySafe(current);
        });
      },
      { defer: true },
    ),
  );

  // desktop keyboard
  onMount(() => {
    const onKey = (event: KeyboardEvent) => {
      if (settings.pagination_mode !== 'paginate') return;
      if (event.key === 'ArrowLeft') void flip(-1);
      if (event.key === 'ArrowRight') void flip(1);
    };
    window.addEventListener('keydown', onKey);
    onCleanup(() => {
      window.removeEventListener('keydown', onKey);
    });
  });

  props.registerNavigator({
    jumpToChapter: position => {
      const section = book?.spine.get(position);
      if (section) void displaySafe(section.href);
    },
    jumpToLocator: locator => {
      if (ready) void displaySafe(locator);
      else pendingLocator = locator;
    },
  });

  return (
    <div class="relative h-full w-full">
      <div
        class="mx-auto h-full w-full"
        ref={container}
        style={{ 'max-width': `${settings.reading_width}em` }}
      />
      <Show when={!ready}>
        <div class="absolute inset-0 flex items-center justify-center">
          <Spinner />
        </div>
      </Show>
    </div>
  );
};

export default EpubReader;
