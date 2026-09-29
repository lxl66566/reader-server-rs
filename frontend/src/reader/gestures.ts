// 阅读器共享手势：tap 判定、tap 区域划分、三段式翻页（strip）触摸状态机。
// TxtReader 分页视图与 PdfReader 共用 useStripFlip；EpubReader 在 iframe 文档上
// 通过 attachTapSwipeGestures 接入同一套 tap/swipe 语义。

import { createSignal, onCleanup, onMount } from 'solid-js';

export const TAP_MAX_MS = 250;
export const TAP_MAX_MOVE_PX = 10;
export const FLIP_THRESHOLD_PX = 60;

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
  onTapCenter: () => void;
  /** 是否翻页模式：滚动模式下 swipe 不翻页，tap 也不分左右区 */
  paginate: () => boolean;
  width: () => number;
}

export type TapZone = 'center' | 'left' | 'right';

interface TouchState {
  id: number;
  moved: boolean;
  startT: number;
  startX: number;
  startY: number;
}

/**
 * 在 epub iframe 文档上挂 touch/click 手势（无跟手动画）：横向滑动超过阈值
 * 翻页，tap 按区域翻页/打开 chrome。返回解绑函数。
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
      handleTap(t0.clientX);
    }
  };

  const handleTap = (clientX: number) => {
    const zone = tapZone(clientX, options.width());
    if (options.paginate()) {
      if (zone === 'left') {
        options.onFlip(-1);
        return;
      }
      if (zone === 'right') {
        options.onFlip(1);
        return;
      }
    }
    options.onTapCenter();
  };

  const onClick = (event: MouseEvent) => {
    handleTap(event.clientX);
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
