// TXT reader: chunked loading + scroll / paginate modes + touch gestures.

import { useNavigate } from '@solidjs/router';
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  Index,
  onCleanup,
  onMount,
  Show,
  untrack,
} from 'solid-js';

import { LoadErrorPanel } from '../components/LoadErrorPanel';
import { useI18n } from '../i18n';
import { fontFamilyCss, useSettings } from '../stores/settings';
import {
  convertChinese,
  convertDirection,
  loadChineseConverter,
} from '../utils/opencc';
import { isTap, useStripFlip } from './gestures';
import { measureCharsPerPage, type PageTypography } from './measure';
import {
  extendWindowDown,
  extendWindowUp,
  jumpWindow,
  type ScrollWindow,
  TextSource,
  TXT_CHUNK_SIZE,
} from './textSource';
import type { FormatReaderProps } from './types';

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

  // --- script conversion: t2s / s2t / none per setting (offsets stay on the original) ---
  // 转换器加载完成后 bump 版本号，触发文本重算
  const [converterVersion, setConverterVersion] = createSignal(0);
  createEffect(() => {
    const direction = convertDirection(settings.chinese_conversion);
    void loadChineseConverter(direction).then(() =>
      setConverterVersion(v => v + 1),
    );
  });
  const displayText = (text: string) => {
    converterVersion();
    return convertChinese(text, convertDirection(settings.chinese_conversion));
  };

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
          {para() || ' '}
        </p>
      )}
    </Index>
  );
};

// 滚动模式中单个 chunk 的段落列表。getParagraphs 对文本未变化的 chunk
// 返回同一数组引用，配合 <For> 的 key 让窗口滑动时未变化的 chunk 不重渲染
const ChunkParagraphs = (props: {
  chunkEls: Map<number, HTMLElement[]>;
  displayText: (text: string) => string;
  getParagraphs: (index: number) => Paragraph[];
  index: number;
  typo: () => PageTypography;
}) => {
  const paragraphs = createMemo(() => props.getParagraphs(props.index));
  const els: HTMLElement[] = [];
  props.chunkEls.set(props.index, els);
  onCleanup(() => {
    if (props.chunkEls.get(props.index) === els) {
      props.chunkEls.delete(props.index);
    }
  });
  return (
    <Index each={paragraphs()}>
      {para => (
        <p
          data-pos={para().offset}
          ref={el => {
            els.push(el);
            onCleanup(() => {
              const i = els.indexOf(el);
              if (i >= 0) els.splice(i, 1);
            });
          }}
          style={{
            'contain-intrinsic-size': `auto ${props.typo().fontSize * props.typo().lineHeight}px`,
            'content-visibility': 'auto',
            margin: `0 0 ${props.typo().paragraphSpacing}em`,
            'text-indent': `${props.typo().textIndent}em`,
            'white-space': 'pre-wrap',
            'word-break': 'break-word',
          }}
        >
          {props.displayText(para().text) || ' '}
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

  // 手势与翻页动画状态（共享 strip 模型）
  const gestures = useStripFlip({
    canNext,
    canPrev,
    onCommit: dir => {
      const cpp = charsPerPage();
      const next = page() + dir;
      props.onOffsetChange(Math.min(next * cpp, props.book.total_chars));
    },
    onTapCenter: () => {
      props.onTapCenter();
    },
    width: () => container?.clientWidth ?? 1,
  });

  const typo = () => props.typography();

  return (
    <div
      class="relative mx-auto h-full w-full overflow-hidden"
      onClick={gestures.onClick}
      onTouchEnd={gestures.onTouchEnd}
      onTouchMove={gestures.onTouchMove}
      onTouchStart={gestures.onTouchStart}
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
            'transition-transform duration-200 ease-out':
              gestures.transitioning(),
          }}
          onTransitionEnd={gestures.onTransitionEnd}
          style={{ transform: gestures.stripTransform(), width: '300%' }}
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

  // 已加载的 chunk 窗口 [start, end)：双向滑动且有上限，避免 DOM 无界增长
  const firstChunk = Math.floor(props.initialOffset / TXT_CHUNK_SIZE);
  const [win, setWin] = createSignal<ScrollWindow>({
    end: firstChunk + 1,
    start: firstChunk,
  });
  const [version, setVersion] = createSignal(0);
  const [loadingMore, setLoadingMore] = createSignal(false);
  const [error, setError] = createSignal(false);
  // 每个 chunk 的段落元素，按 chunk 索引分组；查找时按窗口顺序拼接
  const chunkEls = new Map<number, HTMLElement[]>();

  const totalChunks = () =>
    Math.max(1, Math.ceil(props.book.total_chars / TXT_CHUNK_SIZE));

  // 窗口内的 chunk 索引：只依赖窗口本身，chunk 内容变化由各 chunk 组件自行跟踪
  const chunkIndices = createMemo(() => {
    const { end, start } = win();
    const out: number[] = [];
    for (let i = start; i < end; i++) out.push(i);
    return out;
  });

  // 缓存每个 chunk 的段落拆分结果：文本未变时复用同一数组引用，
  // 配合 <For> 的 key 让未变化的 chunk 完全不重渲染
  const paraCache = new Map<number, { paras: Paragraph[]; text: string }>();

  const chunkParagraphs = (index: number): Paragraph[] => {
    version(); // chunk 数据到达后重算
    const start = index * TXT_CHUNK_SIZE;
    const text = props.source.slice(
      start,
      Math.min(start + TXT_CHUNK_SIZE, props.book.total_chars),
    );
    const cached = paraCache.get(index);
    if (cached && cached.text === text) return cached.paras;
    const paras: Paragraph[] = [];
    let at = start;
    for (const piece of text.split('\n')) {
      paras.push({ offset: at, text: piece });
      at += piece.length + 1; // +1 for the newline
    }
    paraCache.set(index, { paras, text });
    return paras;
  };

  // 窗口滑动后回收缓存，避免占用随书籍体积增长
  createEffect(() => {
    const { end, start } = win();
    for (const key of paraCache.keys()) {
      if (key < start || key >= end) paraCache.delete(key);
    }
  });

  const ensureWindow = async () => {
    const { end, start } = win();
    await props.source.ensureRange(
      start * TXT_CHUNK_SIZE,
      Math.min(end * TXT_CHUNK_SIZE, props.book.total_chars),
    );
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
    // 远距离跳转时重置窗口到目标附近，避免窗口被拉得过大
    const next = jumpWindow(win(), chunk, totalChunks());
    if (next !== win()) setWin(next);
    await ensureWindow();
    requestAnimationFrame(() => {
      const target = findParagraphEl(at);
      if (target && container) {
        container.scrollTop = Math.max(0, target.offsetTop - 12);
      }
    });
  }

  // 按窗口顺序拼接所有段落的 DOM 元素（各 chunk 组内保持 DOM 顺序）
  function paragraphElsInOrder(): HTMLElement[] {
    const { end, start } = win();
    const out: HTMLElement[] = [];
    for (let i = start; i < end; i++) {
      const els = chunkEls.get(i);
      if (els) out.push(...els);
    }
    return out;
  }

  // 二分查找最后一个满足条件的段落（段落的 pos / offsetTop 均随 DOM 顺序单调）
  function lastMatching(
    els: HTMLElement[],
    pred: (el: HTMLElement) => boolean,
  ): HTMLElement | undefined {
    let lo = 0;
    let hi = els.length - 1;
    let best: HTMLElement | undefined;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      const el = els[mid];
      if (!el) break;
      if (pred(el)) {
        best = el;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
    return best;
  }

  // 最后一个 offset <= at 的段落
  function findParagraphEl(at: number): HTMLElement | undefined {
    return lastMatching(
      paragraphElsInOrder(),
      el => Number(el.dataset.pos) <= at,
    );
  }

  function findParagraphElAtTop(scrollTop: number): HTMLElement | undefined {
    if (!container) return undefined;
    const base = container.offsetTop;
    return lastMatching(
      paragraphElsInOrder(),
      el => el.offsetTop - base <= scrollTop + 8,
    );
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
      if (scrollTop < 600 && win().start > 0) void loadPrev();
    });
  };

  // 视口顶部段落作为锚点：窗口滑动后据此恢复滚动位置，避免内容跳动
  function captureAnchor(): null | { delta: number; pos: number } {
    if (!container) return null;
    const el = findParagraphElAtTop(container.scrollTop);
    if (!el) return null;
    return {
      delta: container.scrollTop - (el.offsetTop - container.offsetTop),
      pos: Number(el.dataset.pos),
    };
  }

  function restoreAnchor(anchor: null | { delta: number; pos: number }): void {
    if (!container || !anchor) return;
    const el = findParagraphEl(anchor.pos);
    if (el) {
      container.scrollTop = Math.max(
        0,
        el.offsetTop - container.offsetTop + anchor.delta,
      );
    }
  }

  async function loadNext(): Promise<void> {
    if (loadingMore() || !container) return;
    const prev = win();
    if (prev.end >= totalChunks()) return;
    // 视口已完全越过顶部 chunk 时才回收它，否则允许窗口暂时超限
    const topEl = findParagraphElAtTop(container.scrollTop);
    const canDropTop =
      topEl !== undefined &&
      Math.floor(Number(topEl.dataset.pos) / TXT_CHUNK_SIZE) > prev.start;
    const anchor = captureAnchor();
    setLoadingMore(true);
    setWin(extendWindowDown(prev, totalChunks(), canDropTop));
    try {
      await ensureWindow();
    } catch (err) {
      console.error('Failed to load txt chunk', err);
      // 回滚窗口，下一次滚动触发时会重试
      setWin(prev);
      setLoadingMore(false);
      return;
    }
    // 回收顶部 chunk 后内容上移，按锚点补偿滚动位置
    requestAnimationFrame(() => {
      restoreAnchor(anchor);
      setLoadingMore(false);
    });
  }

  async function loadPrev(): Promise<void> {
    if (loadingMore() || !container) return;
    const prev = win();
    if (prev.start <= 0) return;
    // 视口底部未进入末尾 chunk 时才回收它，否则允许窗口暂时超限
    const bottomEl = findParagraphElAtTop(
      container.scrollTop + container.clientHeight,
    );
    const canDropBottom =
      bottomEl !== undefined &&
      Math.floor(Number(bottomEl.dataset.pos) / TXT_CHUNK_SIZE) < prev.end - 1;
    const anchor = captureAnchor();
    setLoadingMore(true);
    setWin(extendWindowUp(prev, canDropBottom));
    try {
      await ensureWindow();
    } catch (err) {
      console.error('Failed to load txt chunk', err);
      // 回滚窗口，下一次滚动触发时会重试
      setWin(prev);
      setLoadingMore(false);
      return;
    }
    // 前置 chunk 把内容往下推，按锚点恢复滚动位置
    requestAnimationFrame(() => {
      restoreAnchor(anchor);
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
    if (isTap(Date.now() - t, event.clientX - x, event.clientY - y)) {
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
        <For each={chunkIndices()}>
          {index => (
            <ChunkParagraphs
              chunkEls={chunkEls}
              displayText={props.displayText}
              getParagraphs={chunkParagraphs}
              index={index}
              typo={typo}
            />
          )}
        </For>
        <Show when={loadingMore()}>
          <div class="py-4 text-center text-sm opacity-50">{t('loading')}</div>
        </Show>
      </div>
    </Show>
  );
};
