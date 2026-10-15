// d3 堆叠柱状图：x = 时间分桶，y = 阅读时长，每本书一段彩色区间。
// Solid 拥有外层 DOM 与 tooltip，<svg> 内部（数据 join、比例尺、坐标轴、
// 过渡动画）由 d3 接管。移植自 GalgameManager 的 StackedPlaytimeChart，
// 主题配色改为读取本项目的 CSS 变量。
//
// 交互：
// - 悬停整列（含柱条两侧留白）上报 `{ bucketKey, bookId }` 并显示 tooltip；
//   光标压在某本书的分段上时设置 bookId，tooltip 据此淡化其他书。
// - `focusBookId`（来自下方书籍列表的悬停）把该书的分段落到 x 轴上、
//   其余分段变灰上移，列总高不变、无空隙、无需重设比例尺。
import * as d3 from 'd3';
import {
  createEffect,
  createMemo,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
  type Component,
} from 'solid-js';

import { useI18n } from '../../i18n';
import { formatDuration } from '../../utils/format';
import { dateKey, type BucketDatum } from './timeRange';

export interface ChartHover {
  /** 光标下的书；null 表示悬停在空列或列内空白处 */
  bookId: null | number;
  bucketKey: string;
}

export interface ChartSeriesItem {
  bookId: number;
  color: string;
  name: string;
}

interface StackedChartProps {
  class?: string;
  data: BucketDatum[];
  focusBookId: null | number;
  onHover: (hover: ChartHover | null) => void;
  series: ChartSeriesItem[];
}

const MARGIN = { bottom: 32, left: 40, right: 8, top: 22 };
const TRANSITION_MS = 280;

/** 一个渲染后的柱条分段；y0/y1 是自基线起的累计秒数 */
interface Seg {
  bookId: number;
  isTop: boolean;
  key: string;
  v: number;
  y0: number;
  y1: number;
}

interface TickLabel {
  key: string;
  label: string;
}

/** 从 CSS 变量读取图表主题色（与 index.css 的主题色板保持同源） */
function chartTheme(): { axis: string; dim: string; grid: string } {
  const style = getComputedStyle(document.documentElement);
  return {
    axis: style.getPropertyValue('--color-muted').trim(),
    dim: style.getPropertyValue('--color-border').trim(),
    grid: style.getPropertyValue('--color-border').trim(),
  };
}

/**
 * 跟随 <html data-theme> 的主题信号。theme.ts 在 system 模式的媒体查询
 * 翻转路径上没有信号可依赖，用 MutationObserver 覆盖一切切换来源。
 */
function useDataTheme(): () => string {
  const [theme, setTheme] = createSignal(
    document.documentElement.dataset.theme ?? 'light',
  );
  onMount(() => {
    const observer = new MutationObserver(() => {
      setTheme(document.documentElement.dataset.theme ?? 'light');
    });
    observer.observe(document.documentElement, {
      attributeFilter: ['data-theme'],
      attributes: true,
    });
    onCleanup(() => {
      observer.disconnect();
    });
  });
  return theme;
}

const StackedChart: Component<StackedChartProps> = props => {
  const { locale, t } = useI18n();
  const theme = useDataTheme();
  let wrapRef: HTMLDivElement | undefined;
  let svgRef: SVGSVGElement | undefined;
  const [width, setWidth] = createSignal(0);
  // 高度由父级卡片决定，与宽度一样用 ResizeObserver 实测，撑满给定空间
  const [height, setHeight] = createSignal(0);
  const [tip, setTip] = createSignal<null | {
    bookId: null | number;
    bucketKey: string;
    flip: boolean;
    x: number;
    y: number;
  }>(null);

  // 首帧必须立即渲染，仅后续更新走过渡动画
  let isRenderedOnce = false;
  // 去重 onHover 回调：光标在同一段内移动不触发整页重渲染
  let lastHoverKey: null | string = null;

  onMount(() => {
    const ro = new ResizeObserver(entries => {
      const rect = entries[0]?.contentRect;
      if (!rect) return;
      setWidth(Math.max(0, Math.floor(rect.width)));
      setHeight(Math.max(0, Math.floor(rect.height)));
    });
    if (wrapRef) ro.observe(wrapRef);
    onCleanup(() => {
      ro.disconnect();
    });
  });

  createEffect(() => {
    render(
      props.data,
      props.series,
      props.focusBookId,
      width(),
      height(),
      theme(),
      locale(),
    );
  });

  function render(
    data: BucketDatum[],
    series: ChartSeriesItem[],
    focus: null | number,
    w: number,
    h: number,
    _themeSignal: string,
    localeStr: string,
  ) {
    if (!svgRef || w <= 0 || h <= 0 || data.length === 0) return;
    const themeColors = chartTheme();
    const innerW = w - MARGIN.left - MARGIN.right;
    const innerH = h - MARGIN.top - MARGIN.bottom;

    const maxTotal = Math.max(1, d3.max(data, d => d.total) ?? 1);
    const isUseHours = maxTotal >= 3600;
    const factor = isUseHours ? 3600 : 60;
    const unitLabel = isUseHours ? t('statsUnitHour') : t('statsUnitMinute');

    const x = d3
      .scaleBand()
      .domain(data.map(d => d.key))
      .range([0, innerW])
      .padding(0.25);
    const y = d3
      .scaleLinear()
      .domain([0, maxTotal / factor])
      .nice()
      .range([innerH, 0]);

    const colorOf = new Map(series.map(s => [s.bookId, s.color] as const));

    const svg = d3.select(svgRef);
    svg.attr('width', w).attr('height', h).attr('viewBox', `0 0 ${w} ${h}`);
    let root = svg.select<SVGGElement>('g.root');
    if (root.empty()) {
      root = svg.append('g').attr('class', 'root');
      root.append('g').attr('class', 'y-axis');
      root.append('g').attr('class', 'bars');
      root.append('g').attr('class', 'x-axis');
      root.append('g').attr('class', 'overlay');
      root.append('text').attr('class', 'unit-label');
    }
    root.attr('transform', `translate(${MARGIN.left},${MARGIN.top})`);

    const dur = isRenderedOnce ? TRANSITION_MS : 0;

    // ── 堆叠分段 ────────────────────────────────────────────────────────
    // 堆叠顺序：focus 模式下目标书排最前（落到 x 轴基线上），其余按原序
    // 叠在上方，列总高保持不变。
    const order =
      focus == undefined
        ? series
        : [
            ...series.filter(s => s.bookId === focus),
            ...series.filter(s => s.bookId !== focus),
          ];
    const segs: Seg[] = [];
    for (const b of data) {
      let accumulator = 0;
      let last: null | Seg = null;
      for (const s of order) {
        const v = b.perBook.get(s.bookId) ?? 0;
        if (v <= 0) continue;
        const seg: Seg = {
          bookId: s.bookId,
          isTop: false,
          key: b.key,
          v,
          y0: accumulator,
          y1: accumulator + v,
        };
        segs.push(seg);
        last = seg;
        accumulator += v;
      }
      if (last) last.isTop = true;
    }

    // 未聚焦的分段保持累计位置但变灰；y 比例尺不重定基，比例仍可比较
    const segY = (d: Seg): number => y(d.y1 / factor);
    const segH = (d: Seg): number => Math.max(0, innerH - y(d.v / factor));
    const segFill = (d: Seg): string =>
      focus != undefined && d.bookId !== focus
        ? themeColors.dim
        : (colorOf.get(d.bookId) ?? themeColors.dim);
    const segOpacity = (d: Seg): number =>
      focus != undefined && d.bookId !== focus ? 0.4 : 1;
    const segRx = (d: Seg): number =>
      focus == undefined ? (d.isTop ? 2.5 : 0) : d.bookId === focus ? 2.5 : 1;

    root
      .select<SVGGElement>('g.bars')
      .selectAll<SVGRectElement, Seg>('rect')
      .data(segs, d => `${d.key}|${d.bookId}`)
      .join(
        enter => {
          const e = enter
            .append('rect')
            .attr('x', d => x(d.key) ?? 0)
            .attr('width', x.bandwidth())
            .attr('y', innerH)
            .attr('height', 0)
            .attr('opacity', 0)
            .attr('fill', segFill)
            .attr('rx', segRx);
          e.transition()
            .duration(dur)
            .ease(d3.easeCubicOut)
            .attr('y', segY)
            .attr('height', segH)
            .attr('opacity', segOpacity);
          return e;
        },
        update =>
          update
            .transition()
            .duration(dur)
            .ease(d3.easeCubicOut)
            .attr('x', d => x(d.key) ?? 0)
            .attr('width', x.bandwidth())
            .attr('y', segY)
            .attr('height', segH)
            .attr('fill', segFill)
            .attr('opacity', segOpacity)
            .attr('rx', segRx),
        exit =>
          exit
            .transition()
            .duration(dur)
            .attr('y', innerH)
            .attr('height', 0)
            .attr('opacity', 0)
            .remove(),
      );

    // ── y 轴 + 横向网格线 ───────────────────────────────────────────────
    const yAxis = d3.axisLeft(y).ticks(4).tickSize(-innerW).tickPadding(6);
    const gy = root.select<SVGGElement>('g.y-axis');
    if (isRenderedOnce) {
      gy.transition().duration(dur).ease(d3.easeCubicOut).call(yAxis);
    } else {
      gy.call(yAxis);
    }
    gy.select('.domain').attr('stroke', 'none');
    gy.selectAll('.tick line').attr('stroke', themeColors.grid);
    gy.selectAll('.tick text')
      .attr('fill', themeColors.axis)
      .attr('font-size', 11);

    root
      .select<SVGTextElement>('text.unit-label')
      .attr('x', -MARGIN.left + 6)
      .attr('y', -8)
      .attr('fill', themeColors.axis)
      .attr('font-size', 11)
      .text(unitLabel);

    // ── x 轴（自定义刻度标签；密集月份视图稀疏标注） ────────────────────
    const weekdayFmt = new Intl.DateTimeFormat(localeStr, {
      timeZone: 'UTC',
      weekday: 'short',
    });
    const monthFmt = new Intl.DateTimeFormat(localeStr, {
      month: 'short',
      timeZone: 'UTC',
    });
    const isDense = data.length > 10 && data[0]?.unit === 'day';
    // 窄屏抽稀：每个标签约需 48px，超出容量的标签按等间隔丢弃（保留末位）
    const maxTicks = Math.max(1, Math.floor(innerW / 48));
    const tickStep = Math.ceil(data.length / maxTicks);
    const tickLabels: TickLabel[] = data
      .filter(
        (b, i) =>
          i % tickStep === 0 ||
          i === data.length - 1 ||
          (isDense &&
            (b.start.getUTCDate() === 1 ||
              (b.start.getUTCDate() - 1) % 5 === 0)),
      )
      .map(b => {
        if (b.unit === 'month')
          return { key: b.key, label: monthFmt.format(b.start) };
        if (data.length <= 10) {
          // 周视图：'10-14 周三' / '10-14 Wed'
          return {
            key: b.key,
            label: `${dateKey(b.start).slice(5)} ${weekdayFmt.format(b.start)}`,
          };
        }
        // 月视图：每 5 天标一个 '10-01'
        return { key: b.key, label: dateKey(b.start).slice(5) };
      });

    const gx = root.select<SVGGElement>('g.x-axis');
    gx.selectAll<SVGLineElement, number>('line.domain')
      .data([0])
      .join('line')
      .attr('class', 'domain')
      .attr('x1', 0)
      .attr('x2', innerW)
      .attr('y1', innerH + 0.5)
      .attr('y2', innerH + 0.5)
      .attr('stroke', themeColors.grid);

    gx.selectAll<SVGTextElement, TickLabel>('text.tick-label')
      .data(tickLabels, d => d.key)
      .join(
        enter => {
          const e = enter
            .append('text')
            .attr('class', 'tick-label')
            .attr('text-anchor', 'middle')
            .attr('y', innerH + 15)
            .attr('x', d => (x(d.key) ?? 0) + x.bandwidth() / 2)
            .attr('opacity', 0);
          e.transition().duration(dur).attr('opacity', 1);
          return e;
        },
        update =>
          update
            .transition()
            .duration(dur)
            .ease(d3.easeCubicOut)
            .attr('y', innerH + 15)
            .attr('x', d => (x(d.key) ?? 0) + x.bandwidth() / 2)
            .attr('opacity', 1),
        exit => exit.transition().duration(dur).attr('opacity', 0).remove(),
      )
      .attr('fill', themeColors.axis)
      .attr('font-size', 11)
      .text(d => d.label);

    // ── 悬停层：覆盖整个 band step（柱 + 两侧各半留白）的全高矩形 ────────
    const padX = (x.step() - x.bandwidth()) / 2;
    const overlay = root
      .select<SVGGElement>('g.overlay')
      .selectAll<SVGRectElement, BucketDatum>('rect')
      .data(data, d => d.key);

    const overlayMerged = overlay
      .join(enter =>
        enter
          .append('rect')
          .attr('fill', 'transparent')
          .attr('y', 0)
          .attr('height', innerH)
          .attr('x', d => Math.max(0, (x(d.key) ?? 0) - padX))
          .attr(
            'width',
            d =>
              Math.min(innerW, Math.max(0, (x(d.key) ?? 0) - padX) + x.step()) -
              Math.max(0, (x(d.key) ?? 0) - padX),
          ),
      )
      .attr('height', innerH)
      .attr('x', d => Math.max(0, (x(d.key) ?? 0) - padX))
      .attr(
        'width',
        d =>
          Math.min(innerW, Math.max(0, (x(d.key) ?? 0) - padX) + x.step()) -
          Math.max(0, (x(d.key) ?? 0) - padX),
      );

    const clearHover = () => {
      setTip(null);
      if (lastHoverKey !== null) {
        lastHoverKey = null;
        props.onHover(null);
      }
    };

    // 只在指针离开图表或进入边距/刻度区时清除悬停；列间移动不能清除，
    // 否则联动的书籍列表会闪烁
    /* eslint-disable solid/reactivity -- d3 事件处理器，非渲染作用域函数 */
    svg
      .on('mousemove', (event: MouseEvent) => {
        if (event.target === svgRef) clearHover();
      })
      .on('mouseleave', clearHover);

    overlayMerged.on('mousemove', (event: MouseEvent, d) => {
      const node = root.node();
      if (!node) return;
      const [mx, my] = d3.pointer(event, node);

      // 悬停层覆盖整个 band step（含两侧留白），列间移动保持悬停不断。
      // 但只有光标真正压在柱条水平范围内才算"命中"分段；侧留白内即使
      // 低于列顶也只上报无聚焦悬停（bookId = null）。堆叠顺序与渲染一致。
      const barX = x(d.key) ?? 0;
      const isOverBar = mx >= barX && mx <= barX + x.bandwidth();

      let bookId: null | number = null;
      let accumulator = 0;
      if (isOverBar) {
        for (const s of order) {
          const v = d.perBook.get(s.bookId) ?? 0;
          if (v <= 0) continue;
          const yTop = y((accumulator + v) / factor);
          const yBot = y(accumulator / factor);
          if (my >= yTop && my <= yBot) {
            bookId = s.bookId;
            break;
          }
          accumulator += v;
        }
      }

      const bandCenter = MARGIN.left + (x(d.key) ?? 0) + x.bandwidth() / 2;
      setTip({
        bookId,
        bucketKey: d.key,
        flip: bandCenter > innerW / 2,
        x: bandCenter,
        y: MARGIN.top + Math.min(Math.max(my, 0), innerH),
      });
      const hoverKey = `${d.key}|${bookId ?? ''}`;
      if (hoverKey !== lastHoverKey) {
        lastHoverKey = hoverKey;
        props.onHover({ bookId, bucketKey: d.key });
      }
    });
    /* eslint-enable solid/reactivity */

    isRenderedOnce = true;
  }

  // ── tooltip（Solid 渲染的 HTML，跟随光标） ─────────────────────────────
  const tipData = createMemo(() => {
    const ti = tip();
    if (!ti) return null;
    const bucket = props.data.find(b => b.key === ti.bucketKey);
    if (!bucket) return null;
    const rows = props.series
      .map(s => ({ ...s, secs: bucket.perBook.get(s.bookId) ?? 0 }))
      .filter(r => r.secs > 0)
      .toSorted((a, b) => b.secs - a.secs);
    return { ...ti, bucket, rows };
  });

  const tipTitle = (bucket: BucketDatum): string =>
    bucket.unit === 'day'
      ? `${dateKey(bucket.start)} ${new Intl.DateTimeFormat(locale(), {
          timeZone: 'UTC',
          weekday: 'short',
        }).format(bucket.start)}`
      : bucket.key;

  return (
    <div class={`relative h-full w-full ${props.class ?? ''}`} ref={wrapRef}>
      <svg class="block" ref={svgRef} role="img" />
      <Show when={tipData()}>
        {td => (
          <div
            class="border-edge bg-surface pointer-events-none absolute z-10 max-w-80 min-w-36 rounded-lg border px-3 py-2 shadow-lg"
            style={{
              left: `${td().x}px`,
              top: `${td().y}px`,
              transform: `translate(${td().flip ? 'calc(-100% - 10px)' : '10px'}, -50%)`,
            }}
          >
            <div class="text-muted text-xs">{tipTitle(td().bucket)}</div>
            <div class="text-sm font-bold tabular-nums">
              {formatDuration(td().bucket.total, t)}
            </div>
            <div class="mt-1 space-y-0.5">
              <For each={td().rows}>
                {r => {
                  const dimmed = () =>
                    td().bookId != undefined && td().bookId !== r.bookId;
                  return (
                    <div class="flex items-center gap-1.5 text-xs leading-4">
                      <span
                        class="inline-block size-2 shrink-0 rounded-full"
                        style={{
                          'background-color': dimmed()
                            ? 'var(--color-muted)'
                            : r.color,
                        }}
                      />
                      {/* min-w-0 让书名可收缩，时长不被挤换行 */}
                      <span
                        class="min-w-0 truncate"
                        style={{
                          color: dimmed() ? 'var(--color-muted)' : r.color,
                        }}
                      >
                        {r.name}
                      </span>
                      <span
                        class="ml-auto shrink-0 pl-2 font-medium whitespace-nowrap tabular-nums"
                        style={{
                          color: dimmed() ? 'var(--color-muted)' : r.color,
                        }}
                      >
                        {formatDuration(r.secs, t)}
                      </span>
                    </div>
                  );
                }}
              </For>
            </div>
          </div>
        )}
      </Show>
    </div>
  );
};

export default StackedChart;
