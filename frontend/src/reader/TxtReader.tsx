// TXT reader: chunked loading + scroll / paginate modes + touch gestures.

import { useNavigate } from '@solidjs/router';
import {
  createEffect,
  createMemo,
  createSignal,
  Index,
  onCleanup,
  onMount,
  Show,
  untrack,
} from 'solid-js';

import { LoadErrorPanel } from '../components/LoadErrorPanel';
import { useI18n } from '../i18n';
import { fontFamilyCss, useSettings } from '../stores/settings';
import { loadChineseConverter, toSimplified } from '../utils/opencc';
import { measureCharsPerPage, type PageTypography } from './measure';
import { TextSource, TXT_CHUNK_SIZE } from './textSource';
import type { FormatReaderProps } from './types';

const FLIP_THRESHOLD_PX = 60;
const TAP_MAX_MS = 250;
const TAP_MAX_MOVE_PX = 10;
const PAGE_INDICATOR_SPACE_PX = 28;
// 分页重测的去抖间隔：拖动滑块时每个 tick 都改排版，停下后才重测
const MEASURE_DEBOUNCE_MS = 200;

interface Paragraph {
  offset: number;
  text: string;
}

interface ViewProps {
  book: FormatReaderProps['book'];
  displayText: (text: string) => string;
  onTapCenter: () => void;
  readingWidth: () => number;
  source: TextSource;
  typography: () => PageTypography;
}

const TxtReader = (props: FormatReaderProps) => {
  const { settings } = useSettings();
  const source = new TextSource(props.book.book_id, props.book.total_chars);

  // char offset at the top of the viewport (scroll) or page start (paginate)
  const [offset, setOffset] = createSignal(initialOffset());
  const [ready, setReady] = createSignal(false);
  // bridge for navigator jumps while in scroll mode
  const [scrollJump, setScrollJump] = createSignal<null | number>(null);

  function initialOffset(): number {
    const fromLocator = Number.parseInt(props.initialLocator ?? '', 10);
    if (Number.isFinite(fromLocator) && fromLocator >= 0) {
      return Math.min(fromLocator, props.book.total_chars);
    }
    return Math.floor(props.book.percent * props.book.total_chars);
  }

  // --- simplified-chinese display conversion (offsets stay on the original) ---
  const [converterReady, setConverterReady] = createSignal(false);
  createEffect(() => {
    if (settings.simplified_chinese && !converterReady()) {
      void loadChineseConverter().then(() => setConverterReady(true));
    }
  });
  const displayText = (text: string) =>
    settings.simplified_chinese && converterReady() ? toSimplified(text) : text;

  const typography = createMemo((): PageTypography => {
    const family = fontFamilyCss(settings.font_family);
    return {
      fontFamily: family,
      fontSize: settings.font_size,
      letterSpacing: settings.letter_spacing,
      lineHeight: settings.line_height,
      paragraphSpacing: settings.paragraph_spacing,
      textIndent: settings.text_indent,
    };
  });

  const reportPosition = (at: number) => {
    const percent =
      props.book.total_chars > 0 ? at / props.book.total_chars : 0;
    props.onPosition(String(at), Math.min(1, Math.max(0, percent)));
  };

  const jumpToOffset = (at: number) => {
    const clamped = Math.min(Math.max(0, at), props.book.total_chars);
    if (settings.pagination_mode === 'paginate') {
      setOffset(clamped);
    } else {
      setScrollJump(clamped);
    }
  };

  props.registerNavigator({
    jumpToChapter: position => {
      jumpToOffset(position);
    },
    jumpToLocator: locator => {
      const at = Number.parseInt(locator, 10);
      if (Number.isFinite(at)) jumpToOffset(at);
    },
  });

  // report once on ready and on every offset change
  createEffect(() => {
    if (ready()) reportPosition(offset());
  });

  const viewProps = (): ViewProps => ({
    book: props.book,
    displayText,
    onTapCenter: props.onTapCenter,
    readingWidth: () => settings.reading_width,
    source,
    typography,
  });

  return (
    <Show
      fallback={
        <ScrollView
          {...viewProps()}
          initialOffset={offset()}
          jumpRequest={scrollJump}
          onInit={() => setReady(true)}
          onJumpHandled={() => setScrollJump(null)}
          onOffsetChange={setOffset}
        />
      }
      when={settings.pagination_mode === 'paginate'}
    >
      <PaginateView
        {...viewProps()}
        offset={offset()}
        onInit={() => setReady(true)}
        onOffsetChange={setOffset}
      />
    </Show>
  );
};

export default TxtReader;

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

function contentStyle(typo: PageTypography): Record<string, string> {
  return {
    'font-size': `${typo.fontSize}px`,
    'letter-spacing': `${typo.letterSpacing}px`,
    'line-height': String(typo.lineHeight),
    ...(typo.fontFamily ? { 'font-family': typo.fontFamily } : {}),
  };
}

const PageContent = (props: { text: string; typo: PageTypography }) => {
  const paragraphs = createMemo(() => props.text.split('\n'));
  return (
    <Index each={paragraphs()}>
      {para => (
        <p
          style={{
            margin: `0 0 ${props.typo.paragraphSpacing}em`,
            'text-indent': `${props.typo.textIndent}em`,
            'white-space': 'pre-wrap',
            'word-break': 'break-word',
          }}
        >
          {para() || ' '}
        </p>
      )}
    </Index>
  );
};

// ---------------------------------------------------------------------------
// paginate mode
// ---------------------------------------------------------------------------

const PaginateView = (
  props: ViewProps & {
    offset: number;
    onInit: () => void;
    onOffsetChange: (at: number) => void;
  },
) => {
  const { t } = useI18n();
  const navigate = useNavigate();
  let container: HTMLDivElement | undefined;

  const [size, setSize] = createSignal({ height: 0, width: 0 });
  const [charsPerPage, setCharsPerPage] = createSignal(0);
  const [version, setVersion] = createSignal(0); // bump after chunk loads
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal(false);

  // 测量代际：只有最新一代的结果允许落地，避免旧参数的重测覆盖新结果
  let measureGen = 0;
  // 排版/尺寸变化的去抖定时器（拖动滑块时每个 tick 都会触发）
  let measureTimer: ReturnType<typeof setTimeout> | undefined;

  // gesture state
  const [dx, setDx] = createSignal(0);
  const [shift, setShift] = createSignal(0);
  const [transitioning, setTransitioning] = createSignal(false);

  onMount(() => {
    const observer = new ResizeObserver(entries => {
      const entry = entries[0];
      if (!entry) return;
      setSize({
        height: Math.floor(entry.contentRect.height),
        width: Math.floor(entry.contentRect.width),
      });
    });
    if (container) observer.observe(container);
    onCleanup(() => {
      observer.disconnect();
    });
  });

  const onLoadError = (err: unknown) => {
    console.error('Failed to load txt content', err);
    setError(true);
  };

  // 主加载链路：加载样本 chunk 并测量每页字数
  const measure = async () => {
    const gen = ++measureGen;
    const { height, width } = size();
    const typo = props.typography();
    // untracked: re-measuring on every page turn would loop forever
    const start = untrack(() => Math.max(0, props.offset - TXT_CHUNK_SIZE));
    await props.source.ensureRange(start, start + 2 * TXT_CHUNK_SIZE);
    // 等待期间已有更新的测量启动，本次结果作废
    if (gen !== measureGen) return;
    const sample = props.displayText(
      props.source.slice(start, start + 2 * TXT_CHUNK_SIZE),
    );
    if (!sample) return;
    const measured = measureCharsPerPage(
      sample,
      typo,
      width - 32,
      height - PAGE_INDICATOR_SPACE_PX,
    );
    // 2% safety: dense-paragraph text measures taller than the sample
    setCharsPerPage(Math.max(50, Math.floor(measured * 0.98)));
    setLoading(false);
    props.onInit();
  };

  const retry = () => {
    setError(false);
    // 成功后 bump version，让分页文本基于已加载的 chunk 重算
    void measure()
      .then(() => setVersion(v => v + 1))
      .catch(onLoadError);
  };

  // measure chars per page whenever typography / viewport changes
  createEffect(() => {
    const { height, width } = size();
    props.typography();
    // tracking: converted text changes how much fits a page
    props.displayText('x');
    if (width < 50 || height < 50) return;
    // 拖动滑块时每个 tick 都会触发，去抖后只测量最后一组参数
    clearTimeout(measureTimer);
    measureTimer = setTimeout(() => {
      void measure().catch(onLoadError);
    }, MEASURE_DEBOUNCE_MS);
  });
  onCleanup(() => {
    clearTimeout(measureTimer);
  });

  const totalPages = createMemo(() => {
    const cpp = charsPerPage();
    if (cpp <= 0) return 1;
    return Math.max(1, Math.ceil(props.book.total_chars / cpp));
  });

  const page = createMemo(() => {
    const cpp = charsPerPage();
    if (cpp <= 0) return 0;
    return Math.min(totalPages() - 1, Math.floor(props.offset / cpp));
  });

  // text of prev / current / next page
  const pageTexts = createMemo((): string[] => {
    const cpp = charsPerPage();
    version(); // recompute after loads
    if (cpp <= 0) return ['', '', ''];
    const current = page();
    return [-1, 0, 1].map(delta => {
      const p = current + delta;
      if (p < 0 || p >= totalPages()) return '';
      const start = p * cpp;
      return props.displayText(
        props.source.slice(
          start,
          Math.min(start + cpp, props.book.total_chars),
        ),
      );
    });
  });

  // keep the visible trio loaded
  createEffect(() => {
    const cpp = charsPerPage();
    if (cpp <= 0) return;
    const current = page();
    const start = Math.max(0, (current - 1) * cpp);
    const end = Math.min((current + 2) * cpp, props.book.total_chars);
    void props.source
      .ensureRange(start, end)
      .then(() => setVersion(v => v + 1))
      .catch(onLoadError);
  });

  const canPrev = () => page() > 0;
  const canNext = () => page() < totalPages() - 1;

  const flip = (dir: -1 | 1) => {
    if (transitioning()) return;
    if (dir < 0 && !canPrev()) return;
    if (dir > 0 && !canNext()) return;
    setTransitioning(true);
    setShift(dir);
  };

  const onTransitionEnd = () => {
    const moved = shift();
    // batch: transition disabled before the transform jumps back
    setTransitioning(false);
    setShift(0);
    setDx(0);
    if (moved !== 0) {
      const cpp = charsPerPage();
      const next = page() + moved;
      props.onOffsetChange(Math.min(next * cpp, props.book.total_chars));
    }
  };

  const stripTransform = () =>
    `translateX(calc(${-33.3333 * (1 - shift())}% + ${dx()}px))`;

  // --- touch gestures: horizontal drag flips, tap zones flip / open chrome ---
  let touch: null | {
    id: number;
    moved: boolean;
    startT: number;
    startX: number;
    startY: number;
  } = null;

  const onTouchStart = (event: TouchEvent) => {
    const t0 = event.touches[0];
    if (!t0 || transitioning()) return;
    touch = {
      id: t0.identifier,
      moved: false,
      startT: Date.now(),
      startX: t0.clientX,
      startY: t0.clientY,
    };
  };

  const onTouchMove = (event: TouchEvent) => {
    if (!touch) return;
    const t0 = [...event.touches].find(x => x.identifier === touch?.id);
    if (!t0) return;
    const deltaX = t0.clientX - touch.startX;
    const deltaY = t0.clientY - touch.startY;
    if (!touch.moved) {
      if (
        Math.abs(deltaX) < TAP_MAX_MOVE_PX &&
        Math.abs(deltaY) < TAP_MAX_MOVE_PX
      ) {
        return;
      }
      // vertical-dominant movement is not ours (paginate has no vertical scroll)
      if (Math.abs(deltaY) > Math.abs(deltaX)) {
        touch = null;
        return;
      }
      touch.moved = true;
    }
    event.preventDefault();
    let value = deltaX;
    // resistance at the first/last page
    if ((value > 0 && !canPrev()) || (value < 0 && !canNext())) value *= 0.3;
    setDx(value);
  };

  const onTouchEnd = (event: TouchEvent) => {
    if (!touch) return;
    const current = touch;
    touch = null;
    // swallow the synthetic click that follows the touch
    event.preventDefault();
    if (current.moved) {
      const distance = dx();
      if (Math.abs(distance) >= FLIP_THRESHOLD_PX) {
        flip(distance < 0 ? 1 : -1);
      } else {
        // animate the snap back
        setTransitioning(true);
        setDx(0);
      }
      return;
    }
    if (Date.now() - current.startT > TAP_MAX_MS) return;
    const endX = event.changedTouches[0]?.clientX ?? current.startX;
    handleTap(endX);
  };

  const handleTap = (clientX: number) => {
    if (transitioning()) return;
    const width = container?.clientWidth ?? 1;
    const ratio = clientX / width;
    if (ratio < 0.33) flip(-1);
    else if (ratio > 0.67) flip(1);
    else props.onTapCenter();
  };

  // desktop: click zones + keyboard
  const onClick = (event: MouseEvent) => {
    handleTap(event.clientX);
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'ArrowLeft') flip(-1);
    if (event.key === 'ArrowRight') flip(1);
  };

  onMount(() => {
    window.addEventListener('keydown', onKeyDown);
    onCleanup(() => {
      window.removeEventListener('keydown', onKeyDown);
    });
  });

  const typo = () => props.typography();

  return (
    <div
      class="relative mx-auto h-full w-full overflow-hidden"
      onClick={onClick}
      onTouchEnd={onTouchEnd}
      onTouchMove={onTouchMove}
      onTouchStart={onTouchStart}
      ref={container}
      style={{
        'max-width': `${props.readingWidth()}em`,
        'touch-action': 'pan-y',
      }}
    >
      <Show when={!loading() && charsPerPage() > 0}>
        <div
          class="flex h-full"
          classList={{
            'transition-transform duration-200 ease-out': transitioning(),
          }}
          onTransitionEnd={onTransitionEnd}
          style={{ transform: stripTransform(), width: '300%' }}
        >
          <Index each={pageTexts()}>
            {text => (
              <div
                class="h-full w-1/3 overflow-hidden px-4"
                style={contentStyle(typo())}
              >
                <PageContent text={text()} typo={typo()} />
              </div>
            )}
          </Index>
        </div>
        <div class="absolute bottom-1 left-0 w-full text-center text-xs opacity-50">
          {t('readerPageOf', { page: page() + 1, total: totalPages() })}
        </div>
      </Show>
      <Show when={loading() && !error()}>
        <div class="flex h-full items-center justify-center text-sm opacity-60">
          {t('loading')}
        </div>
      </Show>
      <Show when={error()}>
        <div class="absolute inset-0 flex items-center justify-center">
          <LoadErrorPanel
            onBack={() => {
              navigate(`/book/${props.book.book_id}`);
            }}
            onRetry={retry}
          />
        </div>
      </Show>
    </div>
  );
};

// ---------------------------------------------------------------------------
// scroll mode
// ---------------------------------------------------------------------------

const ScrollView = (
  props: ViewProps & {
    initialOffset: number;
    jumpRequest: () => null | number;
    onInit: () => void;
    onJumpHandled: () => void;
    onOffsetChange: (at: number) => void;
  },
) => {
  const { t } = useI18n();
  const navigate = useNavigate();
  let container: HTMLDivElement | undefined;

  // loaded chunk window [startIdx, endIdx)
  const firstChunk = Math.floor(props.initialOffset / TXT_CHUNK_SIZE);
  const [winStart, setWinStart] = createSignal(firstChunk);
  const [winEnd, setWinEnd] = createSignal(firstChunk + 1);
  const [version, setVersion] = createSignal(0);
  const [loadingMore, setLoadingMore] = createSignal(false);
  const [error, setError] = createSignal(false);
  // creation order matches DOM order because <Index> never reorders nodes
  const paragraphEls: HTMLElement[] = [];

  const totalChunks = () =>
    Math.max(1, Math.ceil(props.book.total_chars / TXT_CHUNK_SIZE));

  const paragraphs = createMemo((): Paragraph[] => {
    version();
    const out: Paragraph[] = [];
    for (let i = winStart(); i < winEnd(); i++) {
      const start = i * TXT_CHUNK_SIZE;
      const text = props.source.slice(
        start,
        Math.min(start + TXT_CHUNK_SIZE, props.book.total_chars),
      );
      let at = start;
      for (const piece of text.split('\n')) {
        out.push({ offset: at, text: piece });
        at += piece.length + 1; // +1 for the newline
      }
    }
    return out;
  });

  const ensureWindow = async () => {
    const start = winStart() * TXT_CHUNK_SIZE;
    const end = Math.min(winEnd() * TXT_CHUNK_SIZE, props.book.total_chars);
    await props.source.ensureRange(start, end);
    setVersion(v => v + 1);
  };

  const onLoadError = (err: unknown) => {
    console.error('Failed to load txt content', err);
    setError(true);
  };

  // 主加载链路：失败进入错误态（可重试），而不是永远空白
  const init = async () => {
    await ensureWindow();
    await jumpTo(props.initialOffset);
    props.onInit();
  };

  const retry = () => {
    setError(false);
    void init().catch(onLoadError);
  };

  onMount(() => {
    void init().catch(onLoadError);
  });

  // jumps requested via the navigator (bookmark / chapter / cross-device sync)
  createEffect(() => {
    const at = props.jumpRequest();
    if (at !== null) {
      props.onJumpHandled();
      void jumpTo(at);
    }
  });

  async function jumpTo(at: number): Promise<void> {
    const chunk = Math.floor(at / TXT_CHUNK_SIZE);
    if (chunk < winStart()) setWinStart(chunk);
    if (chunk >= winEnd()) setWinEnd(chunk + 1);
    await ensureWindow();
    requestAnimationFrame(() => {
      const target = findParagraphEl(at);
      if (target && container) {
        container.scrollTop = Math.max(0, target.offsetTop - 12);
      }
    });
  }

  function findParagraphEl(at: number): HTMLElement | undefined {
    // last paragraph whose offset <= at
    let lo = 0;
    let hi = paragraphEls.length - 1;
    let best: HTMLElement | undefined;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      const el = paragraphEls[mid];
      if (!el) break;
      const pos = Number(el.dataset.pos);
      if (pos <= at) {
        best = el;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
    return best;
  }

  let scrollRaf = 0;
  const onScroll = () => {
    cancelAnimationFrame(scrollRaf);
    scrollRaf = requestAnimationFrame(() => {
      if (!container) return;
      const { clientHeight, scrollHeight, scrollTop } = container;
      // report the paragraph at the top of the viewport
      const el = findParagraphElAtTop(scrollTop);
      if (el) props.onOffsetChange(Number(el.dataset.pos));
      // infinite loading in both directions
      if (scrollTop + clientHeight > scrollHeight - 1200) void loadNext();
      if (scrollTop < 600 && winStart() > 0) void loadPrev();
    });
  };

  function findParagraphElAtTop(scrollTop: number): HTMLElement | undefined {
    if (!container) return undefined;
    // paragraphEls follow DOM order, so offsetTop is monotonic: binary search
    const base = container.offsetTop;
    let lo = 0;
    let hi = paragraphEls.length - 1;
    let best: HTMLElement | undefined;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      const el = paragraphEls[mid];
      if (!el) break;
      if (el.offsetTop - base <= scrollTop + 8) {
        best = el;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
    return best;
  }

  async function loadNext(): Promise<void> {
    if (loadingMore() || winEnd() >= totalChunks()) return;
    setLoadingMore(true);
    setWinEnd(winEnd() + 1);
    try {
      await ensureWindow();
    } catch (err) {
      console.error('Failed to load txt chunk', err);
      // 回滚窗口，下一次滚动触发时会重试
      setWinEnd(winEnd() - 1);
    } finally {
      setLoadingMore(false);
    }
  }

  async function loadPrev(): Promise<void> {
    if (loadingMore() || winStart() <= 0) return;
    setLoadingMore(true);
    const prevHeight = container?.scrollHeight ?? 0;
    setWinStart(winStart() - 1);
    try {
      await ensureWindow();
    } catch (err) {
      console.error('Failed to load txt chunk', err);
      setWinStart(winStart() + 1);
      setLoadingMore(false);
      return;
    }
    // keep the viewport pinned: compensate for the prepended chunk height
    requestAnimationFrame(() => {
      if (container) {
        container.scrollTop += container.scrollHeight - prevHeight;
      }
      setLoadingMore(false);
    });
  }

  // tap toggles chrome (scrolling itself is native)
  let downAt: null | { t: number; x: number; y: number } = null;
  const onPointerDown = (event: PointerEvent) => {
    downAt = { t: Date.now(), x: event.clientX, y: event.clientY };
  };
  const onPointerUp = (event: PointerEvent) => {
    if (!downAt) return;
    const { t, x, y } = downAt;
    downAt = null;
    const moved = Math.hypot(event.clientX - x, event.clientY - y);
    if (Date.now() - t < TAP_MAX_MS && moved < TAP_MAX_MOVE_PX) {
      props.onTapCenter();
    }
  };

  const typo = () => props.typography();

  return (
    <Show
      fallback={
        <div class="flex h-full items-center justify-center">
          <LoadErrorPanel
            onBack={() => {
              navigate(`/book/${props.book.book_id}`);
            }}
            onRetry={retry}
          />
        </div>
      }
      when={!error()}
    >
      <div
        class="mx-auto h-full w-full overflow-y-auto px-4 py-6"
        onPointerDown={onPointerDown}
        onPointerUp={onPointerUp}
        onScroll={onScroll}
        ref={container}
        style={{
          'max-width': `${props.readingWidth()}em`,
          ...contentStyle(typo()),
        }}
      >
        <Index each={paragraphs()}>
          {para => (
            <p
              data-pos={para().offset}
              ref={el => {
                paragraphEls.push(el);
                onCleanup(() => {
                  const i = paragraphEls.indexOf(el);
                  if (i >= 0) paragraphEls.splice(i, 1);
                });
              }}
              style={{
                'contain-intrinsic-size': `auto ${typo().fontSize * typo().lineHeight}px`,
                'content-visibility': 'auto',
                margin: `0 0 ${typo().paragraphSpacing}em`,
                'text-indent': `${typo().textIndent}em`,
                'white-space': 'pre-wrap',
                'word-break': 'break-word',
              }}
            >
              {props.displayText(para().text) || ' '}
            </p>
          )}
        </Index>
        <Show when={loadingMore()}>
          <div class="py-4 text-center text-sm opacity-50">{t('loading')}</div>
        </Show>
      </div>
    </Show>
  );
};
