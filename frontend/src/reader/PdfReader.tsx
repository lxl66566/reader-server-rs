// PDF reader: pdf.js canvas rendering, page = locator ("page:N", 0-based),
// 3-page strip with swipe/tap flipping. Pdf has no chapters on the backend.

import { useNavigate } from '@solidjs/router';
import * as pdfjs from 'pdfjs-dist';
import type { PDFDocumentProxy } from 'pdfjs-dist';
import {
  createEffect,
  createSignal,
  Index,
  onCleanup,
  onMount,
  Show,
} from 'solid-js';

import { LoadErrorPanel } from '../components/LoadErrorPanel';
import { Spinner } from '../components/ui';
import { useI18n } from '../i18n';
import { fetchBookFile } from '../offline/api';
import { useSettings } from '../stores/settings';
import { useStripFlip } from './gestures';
import type { FormatReaderProps } from './types';

pdfjs.GlobalWorkerOptions.workerSrc = new URL(
  'pdfjs-dist/build/pdf.worker.min.mjs',
  import.meta.url,
).toString();

const PdfReader = (props: FormatReaderProps) => {
  const { t } = useI18n();
  const navigate = useNavigate();
  let container: HTMLDivElement | undefined;

  const [doc, setDoc] = createSignal<null | PDFDocumentProxy>(null);
  const [page, setPage] = createSignal(initialPage());
  const [ready, setReady] = createSignal(false);
  const [error, setError] = createSignal(false);
  const [pageWidth, setPageWidth] = createSignal(0);

  function initialPage(): number {
    // 深链定位优先于保存的进度；pdf 无章节，忽略章节深链
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
      loadingTask = pdfjs.getDocument({ data: data.slice(0) });
      const loaded = await loadingTask.promise;
      setDoc(loaded);
      setReady(true);
      report(page());
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
  const canPrev = () => page() > 0;
  const canNext = () => page() < totalPages() - 1;

  function report(p: number): void {
    const total = totalPages();
    const percent = total <= 1 ? 1 : p / (total - 1);
    props.onPosition(`page:${p}`, Math.min(1, Math.max(0, percent)));
  }

  const goTo = (p: number) => {
    const clamped = Math.min(Math.max(0, p), totalPages() - 1);
    setPage(clamped);
    report(clamped);
  };

  props.registerNavigator({
    jumpToChapter: () => Promise.resolve(false), // backend has no pdf outline yet
    jumpToLocator: locator => {
      const parsed = parsePageLocator(locator);
      if (parsed === null) return Promise.resolve(false);
      goTo(parsed);
      return Promise.resolve(true);
    },
  });

  // --- flipping (same strip interaction model as the txt paginator) ---
  const gestures = useStripFlip({
    bounds: () => container?.getBoundingClientRect() ?? null,
    canNext,
    canPrev,
    onCommit: dir => {
      goTo(page() + dir);
    },
    onTapCenter: () => {
      props.onTapCenter();
    },
  });

  const stripPages = () => [page() - 1, page(), page() + 1];

  return (
    <div
      class="relative h-full w-full overflow-hidden"
      onClick={event => {
        gestures.handleTap(event.clientX);
      }}
      onTouchEnd={gestures.onTouchEnd}
      onTouchMove={gestures.onTouchMove}
      onTouchStart={gestures.onTouchStart}
      ref={container}
      style={{ 'touch-action': 'pan-y' }}
    >
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
        {doc => (
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
                  <Show when={pageNumber() >= 0 && pageNumber() < totalPages()}>
                    <PdfPageCanvas
                      doc={doc()}
                      pageNumber={pageNumber()}
                      width={pageWidth()}
                    />
                  </Show>
                </div>
              )}
            </Index>
            <div class="absolute bottom-1 left-0 w-full text-center text-xs opacity-50">
              {t('readerPageOf', { page: page() + 1, total: totalPages() })}
            </div>
          </div>
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

// renders one page into a canvas, scaled to the container width
const PdfPageCanvas = (canvasProps: {
  doc: PDFDocumentProxy;
  pageNumber: number;
  width: number;
}) => {
  const { settings } = useSettings();
  let canvas: HTMLCanvasElement | undefined;
  // 串行化渲染队列：新渲染必须等旧 RenderTask settle 后才开始，
  // 否则快速翻页/改宽时会对同一 canvas 并发 render，pdf.js 直接抛错白屏
  let renderChain: Promise<void> = Promise.resolve();

  createEffect(() => {
    const width = Math.min(
      canvasProps.width - 16,
      // reading width caps the canvas like it caps text columns
      settings.reading_width * settings.font_size,
    );
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
