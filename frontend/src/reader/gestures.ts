// 阅读器共享手势：tap 判定、tap 区域划分、三段式翻页（strip）触摸状态机。
// TxtReader 分页视图与 PdfReader 共用 useStripFlip；EpubReader 在 iframe 文档上
// 通过 attachTapSwipeGestures 接入同一套 tap/swipe 语义。

import { createSignal, onCleanup, onMount } from 'solid-js';

export const TAP_MAX_MS = 250;
export const TAP_MAX_MOVE_PX = 10;
export const FLIP_THRESHOLD_PX = 60;
/** 滚动模式点击分区翻页的视口滚动比例 */
export const PAGE_SCROLL_RATIO = 0.8;

const SCROLL_CANCEL_EVENTS = ['wheel', 'touchstart'] as const;
const runningScrolls = new WeakMap<HTMLElement, () => void>();

export interface StripFlipOptions {
  canNext: () => boolean;
  canPrev: () => boolean;
  /** 动画结束、位移归零后提交翻页（dir 为本次动画方向） */
  onCommit: (dir: -1 | 1) => void;
  onTapCenter: () => void;
  /** tap 区域划分的基准宽度（容器宽度） */
  width: () => number;
}

export interface TapSwipeOptions {
  onFlip: (dir: -1 | 1) => void;
  /** 滚动模式：tap 上/下区触发的翻页（章内滚动，章节边界处切章） */
  onPageScroll: (dir: -1 | 1) => void;
  onTapCenter: () => void;
  /** 是否翻页模式：滚动模式下 swipe 不翻页，tap 按纵向分区 */
  paginate: () => boolean;
  /**
   * 事件 clientY 所在坐标系中可视区域的纵向范围。epub iframe 撑满整章内容
   * 高度，clientY 相对整章而非可视区，需由宿主传入滚动偏移换算
   */
  visibleRange: () => { bottom: number; top: number };
  width: () => number;
}

export type TapZone = 'center' | 'left' | 'right';

export type VerticalTapZone = 'bottom' | 'center' | 'top';
interface TouchState {
  id: number;
  moved: boolean;
  startT: number;
  startX: number;
  startY: number;
}

/**
 * 章内翻页的平滑滚动：rAF + ease-out。原生 smooth scrollBy 距离大时
 * 匀速且拖沓，观感卡顿；用户滚轮 / 触摸时立即取消让位给原生滚动，
 * 动画期间再次点击分区也会取消旧动画、从当前位置重新起步。
 */
export function animateScrollBy(
  el: HTMLElement,
  delta: number,
  durationMs = 280,
): void {
  runningScrolls.get(el)?.();
  const start = el.scrollTop;
  const distance =
    Math.min(Math.max(start + delta, 0), el.scrollHeight - el.clientHeight) -
    start;
  if (distance === 0) return;
  const startTime = performance.now();
  let raf = 0;
  let cancelled = false;
  const stop = () => {
    if (cancelled) return;
    cancelled = true;
    cancelAnimationFrame(raf);
    for (const event of SCROLL_CANCEL_EVENTS) {
      el.removeEventListener(event, stop);
    }
    runningScrolls.delete(el);
  };
  runningScrolls.set(el, stop);
  for (const event of SCROLL_CANCEL_EVENTS) {
    el.addEventListener(event, stop, { passive: true });
  }
  const step = (now: number) => {
    if (cancelled) return;
    const t = Math.min(1, (now - startTime) / durationMs);
    el.scrollTop = start + distance * (1 - (1 - t) ** 3);
    if (t < 1) raf = requestAnimationFrame(step);
    else stop();
  };
  raf = requestAnimationFrame(step);
}

/**
 * 在 epub iframe 文档上挂 touch/click 手势（无跟手动画）：横向滑动超过阈值
 * 翻页；tap 在分页模式按左右区翻页、滚动模式按上/中/下区翻页或开 chrome。
 * 返回解绑函数。
 */
export function attachTapSwipeGestures(
  doc: Document,
  options: TapSwipeOptions,
): () => void {
  let start: null | { t: number; x: number; y: number } = null;

  const onTouchStart = (event: TouchEvent) => {
    const t0 = event.touches[0];
    if (t0) start = { t: Date.now(), x: t0.clientX, y: t0.clientY };
  };

  const onTouchEnd = (event: TouchEvent) => {
    if (!start) return;
    const { t, x, y } = start;
    start = null;
    const t0 = event.changedTouches[0];
    if (!t0) return;
    const deltaX = t0.clientX - x;
    const deltaY = t0.clientY - y;
    if (options.paginate()) {
      if (
        Math.abs(deltaX) > FLIP_THRESHOLD_PX &&
        Math.abs(deltaX) > Math.abs(deltaY)
      ) {
        event.preventDefault();
        options.onFlip(deltaX < 0 ? 1 : -1);
        return;
      }
    }
    if (isTap(Date.now() - t, deltaX, deltaY)) {
      event.preventDefault();
      handleTap(t0.clientX, t0.clientY);
    }
  };

  const handleTap = (clientX: number, clientY: number) => {
    if (options.paginate()) {
      const zone = tapZone(clientX, options.width());
      if (zone === 'left') {
        options.onFlip(-1);
        return;
      }
      if (zone === 'right') {
        options.onFlip(1);
        return;
      }
    } else {
      const zone = tapZoneVertical(clientY, options.visibleRange());
      if (zone === 'top') {
        options.onPageScroll(-1);
        return;
      }
      if (zone === 'bottom') {
        options.onPageScroll(1);
        return;
      }
    }
    options.onTapCenter();
  };

  const onClick = (event: MouseEvent) => {
    handleTap(event.clientX, event.clientY);
  };

  doc.addEventListener('touchstart', onTouchStart, { passive: true });
  // touchend 必须非 passive：手势已消费时要 preventDefault 吞掉随后的合成
  // click，否则一次轻点会同时走 touch 与 click 两条路径，handleTap 触发两次。
  // （touchend 阶段的 preventDefault 只抑制合成 click，不影响滚动性能。）
  doc.addEventListener('touchend', onTouchEnd, { passive: false });
  doc.addEventListener('click', onClick);

  return () => {
    doc.removeEventListener('touchstart', onTouchStart);
    doc.removeEventListener('touchend', onTouchEnd);
    doc.removeEventListener('click', onClick);
  };
}

/** 一次按压是否为 tap：短时长且位移在阈值内 */
export function isTap(durationMs: number, dx: number, dy: number): boolean {
  return durationMs < TAP_MAX_MS && Math.hypot(dx, dy) < TAP_MAX_MOVE_PX;
}

/** 按横向位置把 tap 划入左/中/右三区（左 1/3 翻上一页，右 1/3 翻下一页） */
export function tapZone(clientX: number, width: number): TapZone {
  const ratio = clientX / Math.max(1, width);
  if (ratio < 0.33) return 'left';
  if (ratio > 0.67) return 'right';
  return 'center';
}

/** 按纵向位置把 tap 划入上/中/下三区（上 1/3 上翻，下 1/3 下翻，中区开 chrome） */
export function tapZoneVertical(
  clientY: number,
  range: { bottom: number; top: number },
): VerticalTapZone {
  const height = Math.max(1, range.bottom - range.top);
  const ratio = (clientY - range.top) / height;
  if (ratio < 0.33) return 'top';
  if (ratio > 0.67) return 'bottom';
  return 'center';
}

/**
 * 三段式翻页手势：横向拖动跟手（首尾页有阻力），松手超过阈值翻页并播放
 * 位移动画，未超阈值回弹；tap 左/右区翻页、中区打开 chrome；桌面端支持
 * 点击分区与左右方向键。
 */
export function useStripFlip(options: StripFlipOptions) {
  const [dx, setDx] = createSignal(0);
  const [shift, setShift] = createSignal(0);
  const [transitioning, setTransitioning] = createSignal(false);

  let touch: null | TouchState = null;

  const flip = (dir: -1 | 1) => {
    if (transitioning()) return;
    if (dir < 0 && !options.canPrev()) return;
    if (dir > 0 && !options.canNext()) return;
    setTransitioning(true);
    setShift(dir);
  };

  const onTransitionEnd = () => {
    const moved = shift();
    // batch: transition disabled before the transform jumps back
    setTransitioning(false);
    setShift(0);
    setDx(0);
    if (moved !== 0) options.onCommit(moved as -1 | 1);
  };

  const stripTransform = () =>
    `translateX(calc(${-33.3333 * (1 - shift())}% + ${dx()}px))`;

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
    if (
      (value > 0 && !options.canPrev()) ||
      (value < 0 && !options.canNext())
    ) {
      value *= 0.3;
    }
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
    const zone = tapZone(clientX, options.width());
    if (zone === 'left') flip(-1);
    else if (zone === 'right') flip(1);
    else options.onTapCenter();
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

  return {
    dx,
    flip,
    handleTap,
    onClick,
    onTouchEnd,
    onTouchMove,
    onTouchStart,
    onTransitionEnd,
    shift,
    stripTransform,
    transitioning,
  };
}
