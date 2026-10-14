// 阅读器共享手势：tap 判定、tap 区域划分、三段式翻页（strip）触摸状态机、
// 滚动模式的章内翻页与贴边跟踪。
// TxtReader 分页视图与 PdfReader 共用 useStripFlip；EpubReader 在 iframe 文档上
// 通过 attachTapSwipeGestures 接入同一套 tap/swipe 语义。

import { createSignal, onCleanup, onMount } from 'solid-js';

export const TAP_MAX_MS = 250;
export const TAP_MAX_MOVE_PX = 10;
export const FLIP_THRESHOLD_PX = 60;
/** 滚动模式点击分区翻页的视口滚动比例 */
export const PAGE_SCROLL_RATIO = 0.8;

const SCROLL_CANCEL_EVENTS = ['wheel', 'touchstart'] as const;

/** 进行中的滚动动画：start 为动画坐标系下的滚动起点 */
interface RunningScroll {
  cancel: () => void;
  start: number;
}

const runningScrolls = new WeakMap<HTMLElement, RunningScroll>();

export interface StripFlipOptions {
  /** tap 区域划分的基准范围（容器在视口中的实际位置） */
  bounds: () => null | TapBounds;
  canNext: () => boolean;
  canPrev: () => boolean;
  /** 动画结束、位移归零后提交翻页（dir 为本次动画方向） */
  onCommit: (dir: -1 | 1) => void;
  onTapCenter: () => void;
}

/** tap 横向分区的基准范围（容器在视口坐标系中的左右边界） */
export interface TapBounds {
  left: number;
  right: number;
}

export interface TapSwipeOptions {
  /** tap 区域划分的基准范围（容器在视口中的实际位置） */
  bounds: () => null | TapBounds;
  onFlip: (dir: -1 | 1) => void;
  onTapCenter: () => void;
  /** 是否翻页模式：滚动模式下 swipe 不翻页、tap 一律唤出 chrome */
  paginate: () => boolean;
}

export type TapZone = 'center' | 'left' | 'right';

/** 容器范围不可得时的兜底：零宽按中区处理 */
const ZERO_BOUNDS: TapBounds = { left: 0, right: 0 };

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
  runningScrolls.get(el)?.cancel();
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
  const run: RunningScroll = { cancel: stop, start: el.scrollTop };
  const distance =
    Math.min(
      Math.max(run.start + delta, 0),
      el.scrollHeight - el.clientHeight,
    ) - run.start;
  if (distance === 0) return;
  runningScrolls.set(el, run);
  for (const event of SCROLL_CANCEL_EVENTS) {
    el.addEventListener(event, stop, { passive: true });
  }
  const step = (now: number) => {
    if (cancelled) return;
    const t = Math.min(1, (now - startTime) / durationMs);
    el.scrollTop = run.start + distance * (1 - (1 - t) ** 3);
    if (t < 1) raf = requestAnimationFrame(step);
    else stop();
  };
  raf = requestAnimationFrame(step);
}

/**
 * 内容增删导致 scrollTop 被程序化补偿平移后，同步平移进行中的滚动动画
 * 坐标系：step 每帧按旧坐标绝对写回 scrollTop，不跟随平移会整体跳变。
 */
export function shiftRunningScroll(el: HTMLElement, shift: number): void {
  const run = runningScrolls.get(el);
  if (run) run.start += shift;
}

/** 贴边判定容差：距边界 2px 内视为贴边，吸收亚像素滚动取整误差 */
const SCROLL_EDGE_EPSILON_PX = 2;

export interface ScrollEdges {
  bottom: boolean;
  top: boolean;
}

/**
 * 在 epub iframe 文档上挂 touch/click 手势（无跟手动画）：翻页模式下
 * 横向滑动超过阈值翻页、tap 按左右区翻页、中区开 chrome；滚动模式下
 * 滚动交给原生手势，任意 tap 唤出 chrome（iframe 内容高与宿主可视区
 * 的比例换算不可靠，分区翻页曾错位，故不在此分区）。返回解绑函数。
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
    if (!options.paginate()) {
      // 滚动模式：任意 tap 唤出 chrome
      options.onTapCenter();
      return;
    }
    const zone = tapZone(clientX, options.bounds() ?? undefined);
    if (zone === 'left') {
      options.onFlip(-1);
      return;
    }
    if (zone === 'right') {
      options.onFlip(1);
      return;
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

/**
 * 跟踪滚动容器贴边状态并回调，返回解绑函数。仅靠 scroll 事件会在内容
 * 不足一屏（从不产生滚动）时漏判，故同时用 ResizeObserver 跟踪容器与
 * 内容尺寸变化（章节渲染、字号调整）、MutationObserver 跟踪内容子元素
 * 整体替换（epub 换章）。
 */
export function bindScrollEdges(
  el: HTMLElement,
  onChange: (edges: ScrollEdges) => void,
): () => void {
  let raf = 0;
  const schedule = (): void => {
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(() => {
      onChange(scrollEdges(el));
    });
  };
  el.addEventListener('scroll', schedule, { passive: true });

  const observer = new ResizeObserver(schedule);
  const observeChildren = (): void => {
    observer.disconnect();
    observer.observe(el);
    for (const child of Array.from(el.children)) observer.observe(child);
  };
  observeChildren();
  const mutation = new MutationObserver(observeChildren);
  mutation.observe(el, { childList: true });
  schedule();

  return () => {
    cancelAnimationFrame(raf);
    el.removeEventListener('scroll', schedule);
    observer.disconnect();
    mutation.disconnect();
  };
}

/** 一次按压是否为 tap：短时长且位移在阈值内 */
export function isTap(durationMs: number, dx: number, dy: number): boolean {
  return durationMs < TAP_MAX_MS && Math.hypot(dx, dy) < TAP_MAX_MOVE_PX;
}

const EDITABLE_TAGS = new Set(['INPUT', 'SELECT', 'TEXTAREA']);

export interface VerticalTapZoneOptions {
  /** 滚动容器在视口中的纵向范围；不可得时 tap 一律按中区处理 */
  bounds: () => null | { bottom: number; top: number };
  /** 上/下区触发的章内翻页 */
  onPageScroll: (dir: -1 | 1) => void;
  onTapCenter: () => void;
}

/**
 * 键盘事件目标是否为文本输入类元素：这类目标上的按键（含方向键，
 * 如弹窗输入框内移动光标）不应触发翻页。
 */
export function isTextEntryTarget(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || EDITABLE_TAGS.has(target.tagName))
  );
}

/** 滚动容器的贴边状态：内容不足一屏时上下同时贴边 */
export function scrollEdges(el: HTMLElement): ScrollEdges {
  const maxScroll = el.scrollHeight - el.clientHeight;
  return {
    bottom: el.scrollTop >= maxScroll - SCROLL_EDGE_EPSILON_PX,
    top: el.scrollTop <= SCROLL_EDGE_EPSILON_PX,
  };
}

/**
 * 按横向位置把 tap 划入左/中/右三区（左 1/3 翻上一页，右 1/3 翻下一页）。
 * 以容器实际边界为基准：居中列布局下屏幕坐标需先减去容器左偏移，
 * 否则屏幕正中会被误判为右区。零宽/未知范围按中区处理（交给 chrome）。
 */
export function tapZone(
  clientX: number,
  bounds: TapBounds = ZERO_BOUNDS,
): TapZone {
  const width = bounds.right - bounds.left;
  if (width <= 0) return 'center';
  const ratio = (clientX - bounds.left) / width;
  if (ratio < 0.33) return 'left';
  if (ratio > 0.67) return 'right';
  return 'center';
}

/** 按纵向位置把 tap 划入上/中/下三区（上 1/3 上翻，下 1/3 下翻，中区开 chrome） */
export function tapZoneVertical(
  clientY: number,
  range: { bottom: number; top: number },
): VerticalTapZone {
  const height = range.bottom - range.top;
  // 退化范围（未知/零高）按中区处理：唤出 chrome 是最安全的兜底
  if (height <= 0) return 'center';
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
    const zone = tapZone(clientX, options.bounds() ?? undefined);
    if (zone === 'left') flip(-1);
    else if (zone === 'right') flip(1);
    else options.onTapCenter();
  };

  // desktop: click zones + keyboard
  const onClick = (event: MouseEvent) => {
    handleTap(event.clientX);
  };

  const onKeyDown = (event: KeyboardEvent) => {
    // 弹窗输入框内的方向键不翻页
    if (isTextEntryTarget(event.target)) return;
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

/**
 * 滚动模式的纵向 tap 分区：上/下 1/3 章内翻页、中区开 chrome。
 * 触摸路径必须走 touchend——可滚动容器上 pointercancel 会取代 pointerup
 * （浏览器接管滚动手势），pointer 事件判 tap 在触摸设备上完全失效；
 * 桌面走 click，但用 mousedown 位置做 isTap 门控以免文本拖选触发翻页。
 * 触摸消费时 preventDefault 吞掉随后的合成 click，避免重复 dispatch。
 */
export function useVerticalTapZones(options: VerticalTapZoneOptions) {
  let touch: null | { t: number; x: number; y: number } = null;
  let mouse: null | { t: number; x: number; y: number } = null;

  const dispatch = (clientY: number) => {
    const zone = tapZoneVertical(
      clientY,
      options.bounds() ?? {
        bottom: 0,
        top: 0,
      },
    );
    if (zone === 'top') options.onPageScroll(-1);
    else if (zone === 'bottom') options.onPageScroll(1);
    else options.onTapCenter();
  };

  const onMouseDown = (event: MouseEvent) => {
    mouse = { t: Date.now(), x: event.clientX, y: event.clientY };
  };

  const onClick = (event: MouseEvent) => {
    if (!mouse) return;
    const { t, x, y } = mouse;
    mouse = null;
    if (!isTap(Date.now() - t, event.clientX - x, event.clientY - y)) return;
    dispatch(event.clientY);
  };

  const onTouchStart = (event: TouchEvent) => {
    const t0 = event.touches[0];
    if (t0) touch = { t: Date.now(), x: t0.clientX, y: t0.clientY };
  };

  const onTouchEnd = (event: TouchEvent) => {
    if (!touch) return;
    const { t, x, y } = touch;
    touch = null;
    const t0 = event.changedTouches[0];
    if (!t0) return;
    if (!isTap(Date.now() - t, t0.clientX - x, t0.clientY - y)) return;
    event.preventDefault();
    dispatch(t0.clientY);
  };

  return { onClick, onMouseDown, onTouchEnd, onTouchStart };
}
