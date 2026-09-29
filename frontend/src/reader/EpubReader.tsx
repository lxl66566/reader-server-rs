// EPUB reader: epub.js rendition with theme sync, swipe/tap gestures and
// locator = EpubCFI, percent via generated locations.

import { useNavigate } from '@solidjs/router';
import ePub, { type Book, type Rendition } from 'epubjs';
import {
  createEffect,
  createSignal,
  on,
  onCleanup,
  onMount,
  Show,
} from 'solid-js';

import { bookApi } from '../api';
import { LoadErrorPanel } from '../components/LoadErrorPanel';
import { Spinner } from '../components/ui';
import { fontFamilyCss, useSettings } from '../stores/settings';
import {
  type ConvertDirection,
  convertChinese,
  convertDirection,
  loadChineseConverter,
} from '../utils/opencc';
import type { FormatReaderProps } from './types';

const TAP_MAX_MS = 250;
const TAP_MAX_MOVE_PX = 10;
const FLIP_THRESHOLD_PX = 60;

const EpubReader = (props: FormatReaderProps) => {
  const { settings } = useSettings();
  const navigate = useNavigate();
  let container: HTMLDivElement | undefined;

  let book: Book | null = null;
  let rendition: null | Rendition = null;
  // 缓存首次下载的书籍文件：切换翻页模式重建 rendition 时复用，
  // 避免每次切换都重新下载整本 epub（可达几十 MB）
  let cachedData: ArrayBuffer | null = null;
  const [ready, setReady] = createSignal(false);
  const [error, setError] = createSignal(false);
  let pendingLocator: null | string = null;
  let locationsReady = false;
  let lastCfi: null | string = null;

  onMount(() => {
    void runSetup();
    onCleanup(() => {
      setReady(false);
      book?.destroy();
      book = null;
      rendition = null;
    });
  });

  // 主加载链路的统一入口：失败时进入错误态而不是永远停在加载态
  async function runSetup(): Promise<void> {
    setError(false);
    try {
      await setup();
    } catch (err) {
      console.error('Failed to load epub book', err);
      // 销毁半成品实例，让重试从干净状态开始
      book?.destroy();
      book = null;
      rendition = null;
      setReady(false);
      setError(true);
    }
  }

  async function setup(): Promise<void> {
    if (!container) return;
    cachedData ??= await bookApi.fetchFile(props.book.book_id);
    // 每次重建都切一份副本给 epub.js，避免其实例污染共享缓存
    book = ePub(cachedData.slice(0));
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
      try {
        await rendition.display(startAt);
      } catch {
        // 保存的 cfi 可能已过期（如书籍重新处理过）：回退到起始位置
        await rendition.display();
      }
    } else {
      await rendition.display();
    }

    setReady(true);
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
      void loadChineseConverter(
        convertDirection(settings.simplified_chinese),
      ).then(() => {
        if (!rendition) return;
        // 应用时重新读当前设置，避免加载期间切换方向后应用了过期方向
        convertTree(
          contents.document,
          convertDirection(settings.simplified_chinese),
        );
      });
      attachGestures(contents.document);
    });
  }

  // 切换简繁：对已渲染的 iframe 文档就地还原原文再按新方向转换，
  // 不销毁 rendition，避免 clear()+start() 造成的白屏闪烁
  createEffect(
    on(
      () => settings.simplified_chinese,
      simplified => {
        if (!rendition || !ready()) return;
        const direction = convertDirection(simplified);
        void loadChineseConverter(direction).then(() => {
          for (const view of rendition?.views() ?? []) {
            // epub.js 的 View 类型未暴露 contents，但运行时存在
            const doc = (view as { contents?: { document?: Document } })
              .contents?.document;
            if (doc?.body) convertTree(doc, direction);
          }
        });
      },
      { defer: true },
    ),
  );

  // 每个文档首次转换前缓存原文文本节点：来回切换时始终从原文出发，
  // 避免 s2t/t2s 叠加转换造成文字劣化
  const originalTexts = new WeakMap<Document, Map<Text, string>>();

  function convertTree(doc: Document, direction: ConvertDirection): void {
    let originals = originalTexts.get(doc);
    if (!originals) {
      originals = new Map();
      originalTexts.set(doc, originals);
    }
    const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT);
    let node = walker.nextNode();
    while (node) {
      const textNode = node as Text;
      if (!originals.has(textNode))
        originals.set(textNode, textNode.textContent);
      textNode.textContent = convertChinese(
        originals.get(textNode) ?? '',
        direction,
      );
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
    // touchend 必须非 passive：手势已消费时要 preventDefault 吞掉随后的合成
    // click，否则一次轻点会同时走 touch 与 click 两条路径，handleTap 触发两次。
    // （touchend 阶段的 preventDefault 只抑制合成 click，不影响滚动性能。）
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
            event.preventDefault();
            void flip(deltaX < 0 ? 1 : -1);
            return;
          }
        }
        if (
          Date.now() - t < TAP_MAX_MS &&
          Math.hypot(deltaX, deltaY) < TAP_MAX_MOVE_PX
        ) {
          event.preventDefault();
          handleTap(t0.clientX);
        }
      },
      { passive: false },
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
        setReady(false);
        void runSetup().then(() => {
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
      if (ready()) void displaySafe(locator);
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
      <Show when={!ready() && !error()}>
        <div class="absolute inset-0 flex items-center justify-center">
          <Spinner />
        </div>
      </Show>
      <Show when={error()}>
        <div class="absolute inset-0 flex items-center justify-center">
          <LoadErrorPanel
            onBack={() => {
              navigate(`/book/${props.book.book_id}`);
            }}
            onRetry={() => {
              void runSetup();
            }}
          />
        </div>
      </Show>
    </div>
  );
};

export default EpubReader;
