// PDF reader: pdf.js canvas rendering, page = locator ("page:N", 0-based).
// 翻页模式：3-page strip（跟手滑动/点击分区/方向键）；滚动模式：连续页列
// （IntersectionObserver 惰性渲染/释放，上下分区滚动翻页）。后端把书签
// outline 解析为 chapters（position = 0-based 页码）。

import { useNavigate } from '@solidjs/router';
import * as pdfjs from 'pdfjs-dist';
import type { PDFDocumentProxy } from 'pdfjs-dist';
import {
  createEffect,
  createSignal,
  For,
  Index,
  onCleanup,
  onMount,
  Show,
} from 'solid-js';

import { bookApi } from '../api';
import type { Chapter } from '../api/types';
import { LoadErrorPanel } from '../components/LoadErrorPanel';
import { Spinner } from '../components/ui';
import { useI18n } from '../i18n';
import { fetchBookFile } from '../offline/api';
import { useSettings } from '../stores/settings';
import {
  animateScrollBy,
  PAGE_SCROLL_RATIO,
  useStripFlip,
  useVerticalTapZones,
} from './gestures';
import { bindReaderKeys } from './keyboard';
import type { FormatReaderProps } from './types';

pdfjs.GlobalWorkerOptions.workerSrc = new URL(
  'pdfjs-dist/build/pdf.worker.min.mjs',
  import.meta.url,
).toString();

// pdf.js 按需拉取的静态资源（JBIG2/JPX 的 wasm 解码器、CJK CMap、非嵌入
// 字体回退）：vite 插件统一伺服（dev 中间件、build 拷入 dist），缺了它们
// 扫描版整页空白、无嵌入字体的文本页乱码
const PDFJS_ASSETS = '/pdfjs-assets/';

// 画布左右留白（与 txt 分页的 px-4 视觉宽度一致）
const CANVAS_MARGIN_PX = 16;
// 页列的页间距
const PAGE_GAP_PX = 8;
// 惰性渲染半径：进入视口前 1.5 页高度开始渲染，滚出 4 页高度释放位图
const RENDER_AHEAD_RATIOS = 1.5;
const RELEASE_BEHIND_RATIOS = 4;

const PdfReader = (props: FormatReaderProps) => {
  const { settings } = useSettings();
  const navigate = useNavigate();

  const [doc, setDoc] = createSignal<null | PDFDocumentProxy>(null);
  const [ready, setReady] = createSignal(false);
  const [error, setError] = createSignal(false);
  const [page, setPage] = createSignal(initialPage());
  // 阅读区视口宽度（px）驱动画布缩放；resize 时重渲染
  const [pageWidth, setPageWidth] = createSignal(0);
  // 滚动模式跳转桥：ScrollView 串行执行，完成后 resolve（参照 TxtReader）
  const [jumpRequest, setJumpRequest] = createSignal<null | number>(null);
  let jumpDone: ((ok: boolean) => void) | null = null;

  function initialPage(): number {
    // 深链定位优先于保存的进度；章节深链就绪后经目录接口解析页码
    const deep =
      props.deepLink?.kind === 'locator' ? props.deepLink.locator : null;
    const parsed = parsePageLocator(deep ?? props.initialLocator);
    if (parsed !== null) return parsed;
    const total = props.book.total_pages;
    if (total > 0) {
      return Math.min(total - 1, Math.floor(props.book.percent * total));
    }
    return 0;
  }

  let loadingTask: null | pdfjs.PDFDocumentLoadingTask = null;

  // 主加载链路：失败进入错误态（可重试），而不是永远停在加载态
  const load = async () => {
    setError(false);
    // 重试会覆盖 loadingTask：旧 task 先销毁，防 pdf worker 泄漏
    // （仅在卸载时 destroy 覆盖不到加载失败的残留 task）
    void loadingTask?.destroy().catch(() => undefined);
    loadingTask = null;
    try {
      // 断网时回退显式缓存的整本文件
      const data = await fetchBookFile(props.book.book_id);
      // getDocument 会把 buffer transfer 给 worker（原件被 detach），
      // 复制一份保住 ETag 缓存里的原 buffer
      loadingTask = pdfjs.getDocument({
        cMapUrl: `${PDFJS_ASSETS}cmaps/`,
        data: data.slice(0),
        standardFontDataUrl: `${PDFJS_ASSETS}standard_fonts/`,
        wasmUrl: `${PDFJS_ASSETS}wasm/`,
      });
      const loaded = await loadingTask.promise;
      setDoc(loaded);
      setReady(true);
    } catch (err) {
      console.error('Failed to load pdf book', err);
      setReady(false);
      setError(true);
    }
  };

  onMount(() => {
    void load();
    onCleanup(() => {
      // destroy 会连带销毁 worker 线程；只 cleanup 会泄漏 worker，切书后累积
      void loadingTask?.destroy();
    });
  });

  // container width drives the render scale; re-render on resize
  onMount(() => {
    if (!container) return;
    const observer = new ResizeObserver(entries => {
      const entry = entries[0];
      if (entry) setPageWidth(Math.floor(entry.contentRect.width));
    });
    observer.observe(container);
    onCleanup(() => {
      observer.disconnect();
    });
  });

  const totalPages = () =>
    doc()?.numPages ?? Math.max(1, props.book.total_pages);

  // 画布目标宽度：视口宽与阅读宽度上限取小（与文字列宽语义一致）
  const canvasWidth = () =>
    Math.min(
      pageWidth() - CANVAS_MARGIN_PX,
      settings.reading_width * settings.font_size,
    );

  const report = (p: number): void => {
    const total = totalPages();
    const percent = total <= 1 ? 1 : p / (total - 1);
    props.onPosition(`page:${p}`, Math.min(1, Math.max(0, percent)));
  };

  // 位置上报：翻页模式由换页更新 page，滚动模式由 ScrollView 回调更新；
  // 就绪后立即上报一次（心跳与 chrome 百分比依赖它）
  createEffect(() => {
    if (ready()) report(page());
  });

  // 统一跳转入口：翻页模式直接换页；滚动模式桥接进 ScrollView 串行队列
  const jumpToPage = (p: number): Promise<boolean> => {
    const clamped = Math.min(Math.max(0, Math.floor(p)), totalPages() - 1);
    if (settings.pagination_mode === 'paginate') {
      setPage(clamped);
      return Promise.resolve(true);
    }
    if (!ready()) return Promise.resolve(false);
    return new Promise(resolve => {
      jumpDone?.(false); // 被新请求覆盖的旧跳转直接判失败
      jumpDone = resolve;
      setJumpRequest(clamped);
    });
  };

  props.registerNavigator({
    currentChapterPosition: () => page(),
    // 后端 chapters 的 position 即 0-based 页码（书签 outline）
    jumpToChapter: position => jumpToPage(position),
    jumpToLocator: locator => {
      const parsed = parsePageLocator(locator);
      return parsed === null ? Promise.resolve(false) : jumpToPage(parsed);
    },
  });

  // 章节深链（详情页目录）就绪后经后端解析成页码
  let deepLinkDone = false;
  createEffect(() => {
    if (!ready() || deepLinkDone) return;
    deepLinkDone = true;
    const link = props.deepLink;
    if (!link) return;
    if (link.kind === 'locator') {
      const parsed = parsePageLocator(link.locator);
      if (parsed !== null) void jumpToPage(parsed);
      return;
    }
    void bookApi
      .jumpToChapter(props.book.book_id, link.chapterId)
      .then(res => void jumpToPage(res.position))
      .catch(() => undefined); // 章节解析失败：停留在初始位置
  });

  let container: HTMLDivElement | undefined;

  return (
    <div class="relative h-full w-full" ref={container}>
      <Show
        fallback={
          <Show fallback={<SpinnerOverlay />} when={error()}>
            <div class="flex h-full items-center justify-center">
              <LoadErrorPanel
                onBack={() => {
                  navigate(`/book/${props.book.book_id}`);
                }}
                onRetry={() => {
                  void load();
                }}
              />
            </div>
          </Show>
        }
        // 以 doc 本身作条件：函数子组件拿到收窄后的 accessor，免去非空断言
        when={ready() ? doc() : null}
      >
        {loaded => (
          <Show
            fallback={
              <ScrollView
                canvasWidth={canvasWidth()}
                chapters={props.book.chapters}
                doc={loaded()}
                initialPage={page()}
                jumpRequest={jumpRequest}
                onJumpHandled={jump => {
                  const done = jumpDone;
                  jumpDone = null;
                  setJumpRequest(null); // 清空信号：相同目标的再次跳转仍可触发
                  void jump.then(done ?? (() => undefined));
                }}
                onPageChange={setPage}
                onTapCenter={() => {
                  props.onTapCenter();
                }}
              />
            }
            when={settings.pagination_mode === 'paginate'}
          >
            <PaginateView
              canvasWidth={canvasWidth()}
              chapters={props.book.chapters}
              doc={loaded()}
              onTapCenter={() => {
                props.onTapCenter();
              }}
              page={page()}
              setPage={setPage}
            />
          </Show>
        )}
      </Show>
    </div>
  );
};

export default PdfReader;

export function parsePageLocator(locator: null | string): null | number {
  if (!locator) return null;
  const match = /^page:(\d+)$/.exec(locator) ?? /^(\d+)$/.exec(locator);
  if (!match) return null;
  const page = Number(match[1]);
  return Number.isFinite(page) && page >= 0 ? page : null;
}

const SpinnerOverlay = () => (
  <div class="flex h-full items-center justify-center">
    <Spinner />
  </div>
);

// outline 条目的页码可能乱序：当前章 = 列表顺序中最后一个 position <= 页码
// 的条目（与目录面板的定位语义一致），不做提前退出
const chapterIndexAtPage = (
  chapters: readonly Chapter[],
  page: number,
): number => {
  let found = -1;
  for (const [index, chapter] of chapters.entries()) {
    if (chapter.position <= page) found = index;
  }
  return found;
};

// ---------------------------------------------------------------------------
// paginate mode: 3-page strip
// ---------------------------------------------------------------------------

const PaginateView = (viewProps: {
  canvasWidth: number;
  chapters: Chapter[];
  doc: PDFDocumentProxy;
  onTapCenter: () => void;
  page: number;
  setPage: (p: number) => void;
}) => {
  const { t } = useI18n();
  let stripBox: HTMLDivElement | undefined;

  const totalPages = () => viewProps.doc.numPages;
  const canPrev = () => viewProps.page > 0;
  const canNext = () => viewProps.page < totalPages() - 1;

  const goTo = (p: number) => {
    viewProps.setPage(Math.min(Math.max(0, p), totalPages() - 1));
  };

  // --- flipping (same strip interaction model as the txt paginator) ---
  const gestures = useStripFlip({
    bounds: () => stripBox?.getBoundingClientRect() ?? null,
    canNext,
    canPrev,
    onCommit: dir => {
      goTo(viewProps.page + dir);
    },
    onTapCenter: viewProps.onTapCenter,
  });

  // outline 章节 position 即 0-based 页码：键盘切章跳到相邻章节起始页
  const jumpChapter = (dir: -1 | 1): void => {
    const next = chapterIndexAtPage(viewProps.chapters, viewProps.page) + dir;
    if (next < 0) return;
    const target = viewProps.chapters.at(next);
    if (target) goTo(target.position);
  };

  onMount(() => {
    const detachKeys = bindReaderKeys(document, {
      chapter: jumpChapter,
      page: gestures.flip,
    });
    onCleanup(detachKeys);
  });

  const stripPages = () => [
    viewProps.page - 1,
    viewProps.page,
    viewProps.page + 1,
  ];

  return (
    <div
      class="relative h-full w-full overflow-hidden"
      onClick={gestures.onClick}
      onTouchEnd={gestures.onTouchEnd}
      onTouchMove={gestures.onTouchMove}
      onTouchStart={gestures.onTouchStart}
      ref={stripBox}
      style={{ 'touch-action': 'pan-y' }}
    >
      <div
        class="flex h-full"
        classList={{
          'transition-transform duration-200 ease-out':
            gestures.transitioning(),
        }}
        onTransitionEnd={gestures.onTransitionEnd}
        style={{ transform: gestures.stripTransform(), width: '300%' }}
      >
        <Index each={stripPages()}>
          {pageNumber => (
            <div class="flex h-full w-1/3 items-start justify-center overflow-hidden">
              <Show
                when={
                  pageNumber() >= 0 && pageNumber() < totalPages()
                    ? viewProps.doc
                    : null
                }
              >
                {doc => (
                  <PdfPageCanvas
                    doc={doc()}
                    pageNumber={pageNumber()}
                    width={viewProps.canvasWidth}
                  />
                )}
              </Show>
            </div>
          )}
        </Index>
        <div class="absolute bottom-1 left-0 w-full text-center text-xs opacity-50">
          {t('readerPageOf', {
            page: viewProps.page + 1,
            total: totalPages(),
          })}
        </div>
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// scroll mode: continuous page column with lazy render
// ---------------------------------------------------------------------------

// 单页占位 + 惰性渲染包装：双 IntersectionObserver 做滞回（近圈 1.5 视口高
// 内挂载画布，远圈 4 视口高外卸载释放位图）；宽高比实测后回填给页列，
// 供未渲染页估算占位高度
const PageSlot = (slotProps: {
  doc: PDFDocumentProxy;
  index: number;
  onRatio: (index: number, ratio: number) => void;
  root: HTMLDivElement | undefined;
  width: number;
}) => {
  let holder: HTMLDivElement | undefined;
  let inner: HTMLDivElement | undefined;
  const [near, setNear] = createSignal(false);
  // 尚未被 far 观察器看到过时视为远处：初始不渲染
  const [far, setFar] = createSignal(true);

  onMount(() => {
    const root = slotProps.root;
    if (!holder || !root) return;
    const viewportHeight = root.clientHeight ? root.clientHeight : 800;
    const nearObserver = new IntersectionObserver(
      entries => {
        for (const entry of entries) setNear(entry.isIntersecting);
      },
      {
        root,
        rootMargin: `${Math.round(viewportHeight * RENDER_AHEAD_RATIOS)}px 0px`,
      },
    );
    const farObserver = new IntersectionObserver(
      entries => {
        for (const entry of entries) setFar(!entry.isIntersecting);
      },
      {
        root,
        rootMargin: `${Math.round(viewportHeight * RELEASE_BEHIND_RATIOS)}px 0px`,
      },
    );
    nearObserver.observe(holder);
    farObserver.observe(holder);
    onCleanup(() => {
      nearObserver.disconnect();
      farObserver.disconnect();
    });
  });

  // canvas 展示尺寸（首渲染/换宽）→ 宽高比回填
  onMount(() => {
    const el = inner;
    if (!el) return;
    const measure = () => {
      if (el.clientWidth > 0 && el.clientHeight > 0) {
        slotProps.onRatio(slotProps.index, el.clientHeight / el.clientWidth);
      }
    };
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    onCleanup(() => {
      observer.disconnect();
    });
  });

  return (
    <div
      class="flex h-full w-full items-start justify-center overflow-hidden"
      ref={holder}
    >
      <Show when={near() || !far()}>
        <div class="inline-block" ref={inner}>
          <PdfPageCanvas
            doc={slotProps.doc}
            pageNumber={slotProps.index}
            width={slotProps.width}
          />
        </div>
      </Show>
    </div>
  );
};

const ScrollView = (viewProps: {
  canvasWidth: number;
  chapters: Chapter[];
  doc: PDFDocumentProxy;
  initialPage: number;
  jumpRequest: () => null | number;
  onJumpHandled: (jump: Promise<boolean>) => void;
  onPageChange: (p: number) => void;
  onTapCenter: () => void;
}) => {
  let container: HTMLDivElement | undefined;

  // 已测页的宽高比（canvas 高/宽）：估算未渲染页占位高度用
  const ratios = new Map<number, number>();
  const [ratiosVersion, bumpRatios] = createSignal(0);
  // 页占位元素：offsetTop 二分与跳转定位用
  const pageEls = new Map<number, HTMLElement>();

  const total = () => viewProps.doc.numPages;
  const pageIndices = () => Array.from({ length: total() }, (_, i) => i);

  const estimateHeight = (index: number): number => {
    const width = viewProps.canvasWidth;
    if (width <= 0) return PAGE_GAP_PX;
    ratiosVersion();
    const ratio =
      ratios.get(index) ?? ratios.values().next().value ?? Math.SQRT2;
    return Math.round(width * ratio) + PAGE_GAP_PX;
  };

  const setRatio = (index: number, ratio: number): void => {
    if (!Number.isFinite(ratio) || ratio <= 0) return;
    if (ratios.get(index) === ratio) return;
    ratios.set(index, ratio);
    bumpRatios(v => v + 1);
  };

  // 视口顶页 = 最后一个 offsetTop - base <= scrollTop 的页（offsetTop 随
  // DOM 顺序与页高单调递增，二分查找）
  const topPage = (): number => {
    if (!container) return 0;
    const base = container.offsetTop;
    const scrollTop = container.scrollTop;
    let lo = 0;
    let hi = total() - 1;
    let best = 0;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      const el = pageEls.get(mid);
      if (!el) break;
      if (el.offsetTop - base <= scrollTop) {
        best = mid;
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
    return best;
  };

  let scrollRaf = 0;
  const onScroll = () => {
    cancelAnimationFrame(scrollRaf);
    scrollRaf = requestAnimationFrame(() => {
      viewProps.onPageChange(topPage());
    });
  };
  onCleanup(() => {
    cancelAnimationFrame(scrollRaf);
  });

  // 跳转与初始化串行执行：并发 jumpTo 会竞争 scrollTop
  let jumpChain: Promise<boolean> = Promise.resolve(false);

  const jumpTo = async (p: number): Promise<void> => {
    const box = container;
    if (!box) return;
    const land = (): void => {
      const el = pageEls.get(p);
      if (el) {
        box.scrollTop = Math.max(0, el.offsetTop - box.offsetTop);
      }
    };
    land();
    // 页高从估算修正为实测后 offsetTop 会变，下一帧再校正一次
    await new Promise<void>(resolve => {
      requestAnimationFrame(() => {
        land();
        resolve();
      });
    });
  };

  const enqueueJump = (p: number): Promise<boolean> => {
    const run = (): Promise<boolean> =>
      jumpTo(p).then(
        () => true,
        () => false,
      );
    // 链上前序失败不影响本次执行
    const job = jumpChain.then(run, run);
    jumpChain = job;
    return job;
  };

  onMount(() => {
    void enqueueJump(viewProps.initialPage);
    // 键盘：空格/enter/上下键滚动视口，左右方向键切章
    const detachKeys = bindReaderKeys(document, {
      chapter: jumpChapter,
      page: pageScroll,
    });
    onCleanup(detachKeys);
  });

  // jumps requested via the navigator (bookmark / chapter / cross-device sync)
  createEffect(() => {
    const p = viewProps.jumpRequest();
    if (p !== null) {
      viewProps.onJumpHandled(enqueueJump(p));
    }
  });

  // 点击分区与键盘共用：滚动 PAGE_SCROLL_RATIO 视口（贴边自然停住）
  const pageScroll = (dir: -1 | 1): void => {
    if (container) {
      animateScrollBy(
        container,
        dir * container.clientHeight * PAGE_SCROLL_RATIO,
      );
    }
  };

  // outline 章节 position 即 0-based 页码：键盘切章跳到相邻章节起始页
  const jumpChapter = (dir: -1 | 1): void => {
    const next = chapterIndexAtPage(viewProps.chapters, topPage()) + dir;
    if (next < 0) return;
    const target = viewProps.chapters.at(next);
    if (target) void enqueueJump(target.position);
  };

  // 点击分区：上/下 1/3 章内滚动，中区开 chrome
  const tapZones = useVerticalTapZones({
    bounds: () => container?.getBoundingClientRect() ?? null,
    onPageScroll: pageScroll,
    onTapCenter: viewProps.onTapCenter,
  });

  return (
    <div
      class="h-full w-full overflow-y-auto"
      onClick={tapZones.onClick}
      onMouseDown={tapZones.onMouseDown}
      onScroll={onScroll}
      onTouchEnd={tapZones.onTouchEnd}
      onTouchStart={tapZones.onTouchStart}
      ref={container}
    >
      <div
        class="mx-auto w-full"
        style={{ width: `${viewProps.canvasWidth}px` }}
      >
        <For each={pageIndices()}>
          {index => (
            <div
              ref={el => {
                pageEls.set(index, el);
                onCleanup(() => {
                  if (pageEls.get(index) === el) pageEls.delete(index);
                });
              }}
              style={{ height: `${estimateHeight(index)}px` }}
            >
              <PageSlot
                doc={viewProps.doc}
                index={index}
                onRatio={setRatio}
                root={container}
                width={viewProps.canvasWidth}
              />
            </div>
          )}
        </For>
      </div>
    </div>
  );
};

// renders one page into a canvas, scaled to the given css width
const PdfPageCanvas = (canvasProps: {
  doc: PDFDocumentProxy;
  pageNumber: number;
  width: number;
}) => {
  let canvas: HTMLCanvasElement | undefined;
  // 串行化渲染队列：新渲染必须等旧 RenderTask settle 后才开始，
  // 否则快速翻页/改宽时会对同一 canvas 并发 render，pdf.js 直接抛错白屏
  let renderChain: Promise<void> = Promise.resolve();

  createEffect(() => {
    const width = canvasProps.width;
    const doc = canvasProps.doc;
    const pageNumber = canvasProps.pageNumber;
    if (width < 50 || !canvas) return;
    // object ref defeats TS flow narrowing (it can't see the cleanup writes)
    const state = { cancelled: false };
    let task: null | pdfjs.RenderTask = null;
    const run = async () => {
      const page = await doc.getPage(pageNumber + 1); // pdf.js is 1-based
      const base = page.getViewport({ scale: 1 });
      const dpr = Math.min(window.devicePixelRatio, 2);
      const scale = (width / base.width) * dpr;
      const viewport = page.getViewport({ scale });
      const target = canvas;
      if (state.cancelled) return;
      target.width = Math.floor(viewport.width);
      target.height = Math.floor(viewport.height);
      target.style.width = `${Math.floor(viewport.width / dpr)}px`;
      const context = target.getContext('2d');
      if (!context) return;
      task = page.render({ canvas: target, canvasContext: context, viewport });
      await task.promise;
    };
    renderChain = renderChain
      .catch(() => undefined)
      .then(run)
      .catch((err: unknown) => {
        // 翻页/改宽触发的取消是正常路径，其余错误才上报
        if (
          state.cancelled ||
          err instanceof pdfjs.RenderingCancelledException
        ) {
          return;
        }
        console.error('Failed to render pdf page', err);
      });
    onCleanup(() => {
      state.cancelled = true;
      task?.cancel();
    });
  });

  return <canvas class="block shadow-none" ref={canvas} />;
};
