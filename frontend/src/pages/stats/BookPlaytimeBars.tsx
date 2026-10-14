// 分书籍时长横向条形列表：[色点] [书名 +（细条 + 时长）]。
// 条长与当前统计范围内该书的时长成正比。保持 HTML + CSS 过渡（不用 d3）：
// 这是富文本行列表，SVG 只会增加负担。移植自 GalgameManager 的
// GamePlaytimeBars，封面缩略图改为色点（本项目无封面接口）。
//
// 联动：
// - 悬停行回调 `onHoverBook`，驱动堆叠图的 focus 模式；
// - `highlightBookId`（堆叠图分段被悬停）保留该行、淡化其余行，并自动
//   滚动到可见——列表滚动时也能立即看到联动行；
// - 行尾编辑按钮回调 `onEditBook`（可选），打开该书每日时长编辑弹窗。
import { VsEdit } from 'solid-icons/vs';
import { createEffect, createMemo, For, Show, type Component } from 'solid-js';

import { useI18n } from '../../i18n';
import { formatDuration } from '../../utils/format';

export interface BookBarRow {
  bookId: number;
  color: string;
  name: string;
  secs: number;
}

interface BookPlaytimeBarsProps {
  class?: string;
  emptyText: string;
  highlightBookId: null | number;
  /** 打开某书每日时长编辑弹窗；缺省时不渲染编辑按钮 */
  onEditBook?: (id: number) => void;
  onHoverBook: (id: null | number) => void;
  rows: BookBarRow[];
}

const BookPlaytimeBars: Component<BookPlaytimeBarsProps> = props => {
  const { t } = useI18n();
  // memo 化：每次范围变化只扫一遍列表，避免每行宽度计算都全表扫描
  const maxSecs = createMemo(() => Math.max(1, ...props.rows.map(r => r.secs)));

  // 堆叠图高亮某本书时把对应行滚入视野。scrollIntoView({ block: 'nearest' })
  // 在行已可见时是 no-op，只滚动最小距离，且只作用于最近的滚动祖先
  let rootRef: HTMLDivElement | undefined;
  createEffect(() => {
    const id = props.highlightBookId;
    if (id == undefined || !rootRef) return;
    // 用 querySelector 而非逐行 ref：行对象在范围切换时整体重建
    const element = rootRef.querySelector<HTMLElement>(
      `[data-book-id="${CSS.escape(String(id))}"]`,
    );
    if (element)
      element.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
  });

  return (
    <div class={`flex flex-col gap-1 ${props.class ?? ''}`} ref={rootRef}>
      <Show
        fallback={
          <div class="text-muted py-6 text-center text-sm">
            {props.emptyText}
          </div>
        }
        when={props.rows.length > 0}
      >
        <For each={props.rows}>
          {row => {
            const dimmed = () =>
              props.highlightBookId != undefined &&
              props.highlightBookId !== row.bookId;
            return (
              <div
                class="hover:bg-page flex cursor-default items-center gap-3 rounded-md px-2 py-1.5 transition-opacity duration-200"
                classList={{ 'opacity-40': dimmed() }}
                data-book-id={row.bookId}
                onMouseEnter={() => {
                  props.onHoverBook(row.bookId);
                }}
                onMouseLeave={() => {
                  props.onHoverBook(null);
                }}
              >
                <span
                  class="size-3 shrink-0 rounded-full"
                  style={{ 'background-color': row.color }}
                />
                <div class="flex min-w-0 flex-1 flex-col gap-1">
                  <span
                    class="truncate text-sm"
                    style={{ color: row.color }}
                    title={row.name}
                  >
                    {row.name}
                  </span>
                  <div class="flex items-center gap-2">
                    <div
                      class="h-1.5 min-w-3 rounded-full transition-[width] duration-300 ease-out"
                      style={{
                        'background-color': row.color,
                        width: `${(row.secs / maxSecs()) * 100}%`,
                      }}
                    />
                    <span class="text-muted shrink-0 text-xs tabular-nums">
                      {formatDuration(row.secs, t)}
                    </span>
                  </div>
                </div>
                <Show when={props.onEditBook}>
                  <button
                    aria-label={t('statsEditDaily')}
                    class="text-muted hover:bg-edge hover:text-ink shrink-0 cursor-pointer rounded p-1 transition-colors"
                    onClick={() => {
                      props.onEditBook?.(row.bookId);
                    }}
                    title={t('statsEditDaily')}
                    type="button"
                  >
                    <VsEdit class="size-3.5" />
                  </button>
                </Show>
              </div>
            );
          }}
        </For>
      </Show>
    </div>
  );
};

export default BookPlaytimeBars;
