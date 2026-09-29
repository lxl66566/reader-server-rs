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

import { bookApi } from '../api';
import { LoadErrorPanel } from '../components/LoadErrorPanel';
import { Spinner } from '../components/ui';
import { useI18n } from '../i18n';
import { useSettings } from '../stores/settings';
import type { FormatReaderProps } from './types';

const TAP_MAX_MS = 250;
const TAP_MAX_MOVE_PX = 10;
const FLIP_THRESHOLD_PX = 60;

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

  // gesture / animation state (same strip model as the txt paginator)
  const [dx, setDx] = createSignal(0);
  const [shift, setShift] = createSignal(0);
  const [transitioning, setTransitioning] = createSignal(false);

  function initialPage(): number {
    const parsed = parsePageLocator(props.initialLocator);
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
    try {
      const data = await bookApi.fetchFile(props.book.book_id);
      loadingTask = pdfjs.getDocument({ data });
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
    jumpToChapter: () => undefined, // backend has no pdf outline yet
    jumpToLocator: locator => {
      const parsed = parsePageLocator(locator);
      if (parsed !== null) goTo(parsed);
    },
  });

  // --- flipping (same interaction model as the txt paginator) ---
  const flip = (dir: -1 | 1) => {
    if (transitioning()) return;
    if (dir < 0 && !canPrev()) return;
    if (dir > 0 && !canNext()) return;
    setTransitioning(true);
    setShift(dir);
  };

  const onTransitionEnd = () => {
    const moved = shift();
    setTransitioning(false);
    setShift(0);
    setDx(0);
    if (moved !== 0) goTo(page() + moved);
  };

  const stripTransform = () =>
    `translateX(calc(${-33.3333 * (1 - shift())}% + ${dx()}px))`;

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
      if (Math.abs(deltaY) > Math.abs(deltaX)) {
        touch = null;
        return;
      }
      touch.moved = true;
    }
    event.preventDefault();
    let value = deltaX;
    if ((value > 0 && !canPrev()) || (value < 0 && !canNext())) value *= 0.3;
    setDx(value);
  };

  const onTouchEnd = (event: TouchEvent) => {
    if (!touch) return;
    const current = touch;
    touch = null;
    event.preventDefault();
    if (current.moved) {
      const distance = dx();
      if (Math.abs(distance) >= FLIP_THRESHOLD_PX) {
        flip(distance < 0 ? 1 : -1);
      } else {
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

  const stripPages = () => [page() - 1, page(), page() + 1];

  return (
    <div
      class="relative h-full w-full overflow-hidden"
      onClick={event => {
        handleTap(event.clientX);
      }}
      onTouchEnd={onTouchEnd}
      onTouchMove={onTouchMove}
      onTouchStart={onTouchStart}
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
        when={ready() && doc()}
      >
        <div
          class="flex h-full"
          classList={{
            'transition-transform duration-200 ease-out': transitioning(),
          }}
          onTransitionEnd={onTransitionEnd}
          style={{ transform: stripTransform(), width: '300%' }}
        >
          <Index each={stripPages()}>
            {pageNumber => (
              <div class="flex h-full w-1/3 items-start justify-center overflow-hidden">
                <Show when={pageNumber() >= 0 && pageNumber() < totalPages()}>
                  <PdfPageCanvas
                    doc={doc()!}
                    pageNumber={pageNumber()}
                    width={pageWidth()}
                  />
                </Show>
              </div>
            )}
          </Index>
        </div>
        <div class="absolute bottom-1 left-0 w-full text-center text-xs opacity-50">
          {t('readerPageOf', { page: page() + 1, total: totalPages() })}
        </div>
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
