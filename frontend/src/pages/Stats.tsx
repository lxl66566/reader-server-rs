import { useSearchParams } from '@solidjs/router';
// 阅读统计页：汇总卡片 + 周/月/年堆叠柱状图，图表与分书籍时长列表
// 双向联动（悬停列 → 列表过滤到当日；悬停行 → 图表聚焦该书）。
// 图表结构移植自 GalgameManager 统计页，配色使用金色角色轮。
import {
  VsChevronLeft,
  VsChevronRight,
  VsFlame,
  VsRefresh,
  VsTarget,
  VsWatch,
} from 'solid-icons/vs';
import {
  createEffect,
  createMemo,
  createResource,
  createSignal,
  For,
  on,
  onCleanup,
  onMount,
  Show,
  type Component,
  type JSX,
  type ParentComponent,
} from 'solid-js';

import { ApiError, readingApi } from '../api';
import type { ReadingStats } from '../api/types';
import {
  Button,
  EmptyState,
  IconButton,
  Modal,
  SectionCard,
  Spinner,
} from '../components/ui';
import { useI18n } from '../i18n';
import type { DictKey } from '../i18n/zh-CN';
import { useRequireAuth } from '../stores/auth';
import { useSettings } from '../stores/settings';
import { formatDuration } from '../utils/format';
import BookPlaytimeBars, { type BookBarRow } from './stats/BookPlaytimeBars';
import { goldenColor } from './stats/colors';
import DailyEditModal from './stats/DailyEditModal';
import StackedChart, {
  type ChartHover,
  type ChartSeriesItem,
} from './stats/StackedChart';
import {
  addDays,
  aggregate,
  dateKey,
  localeFirstWeekday,
  offsetForDate,
  perBookTotals,
  rangeLabel,
  resolveSelection,
  todayKeyIn,
  type Granularity,
} from './stats/timeRange';

const GRANULARITIES: Granularity[] = ['week', 'month', 'year'];

// 周期总时长标签的 key（编译期枚举，避免拼接字符串键）
const PERIOD_KEYS: Record<Granularity, { current: DictKey; other: DictKey }> = {
  month: {
    current: 'statsPeriodThisMonth',
    other: 'statsPeriodThatMonth',
  },
  week: { current: 'statsPeriodThisWeek', other: 'statsPeriodThatWeek' },
  year: { current: 'statsPeriodThisYear', other: 'statsPeriodThatYear' },
};

const Stats: Component = () => {
  useRequireAuth();
  const { locale, t } = useI18n();
  const { settings } = useSettings();

  // 隐藏文档级滚动条但保留滚动能力；仅统计页生效，离开时恢复
  onMount(() => {
    document.documentElement.classList.add('scrollbar-none');
    onCleanup(() => {
      document.documentElement.classList.remove('scrollbar-none');
    });
  });

  // ── 汇总卡片 ──
  const [statsError, setStatsError] = createSignal<null | string>(null);
  const [stats, { refetch }] = createResource(async () => {
    try {
      const data = await readingApi.stats();
      setStatsError(null);
      return data;
    } catch (error) {
      setStatsError(
        error instanceof ApiError ? error.message : t('errorLoadFailed'),
      );
      throw error;
    }
  });

  // ── 区间选择 ──
  const [granularity, setGranularity] = createSignal<Granularity>('week');
  const [offset, setOffset] = createSignal(0);
  const [pickerOpen, setPickerOpen] = createSignal(false);

  // ── 每日时长编辑弹窗 ──
  // 由分书籍列表行尾按钮或 `?editBook=<id>` 深链（详情页“修改阅读时长”
  // 弹窗的跳转链接）打开
  const [editingBookId, setEditingBookId] = createSignal<null | number>(null);
  const [searchParams, setSearchParams] = useSearchParams();
  createEffect(() => {
    // 数组形态需收窄：同名参数可能重复出现
    const raw = searchParams.editBook;
    const value = Array.isArray(raw) ? raw[0] : raw;
    if (!value) return;
    const id = Number(value);
    if (!Number.isInteger(id) || id <= 0) return;
    // 清掉参数：关闭弹窗后刷新/回退不再重复打开
    setSearchParams({ editBook: undefined }, { replace: true });
    setEditingBookId(id);
  });

  // 统计日期语义跟随设置中的时区（与服务端日报归算一致），默认本地时区
  const weekFirstDay = createMemo(() => localeFirstWeekday(locale()));
  const todayKey = createMemo(() => todayKeyIn(settings.timezone));
  const range = createMemo(() =>
    resolveSelection(
      { granularity: granularity(), offset: offset() },
      weekFirstDay(),
      todayKey(),
    ),
  );
  const rangeParams = createMemo(() => ({
    end: dateKey(addDays(range().end, -1)),
    start: dateKey(range().start),
  }));

  // ── 区间明细 ──
  const [daily, dailyActions] = createResource(rangeParams, params =>
    readingApi.dailyStats(params.start, params.end),
  );

  const bucketData = createMemo(() =>
    aggregate(daily()?.days ?? [], range().buckets),
  );
  const rangeTotal = createMemo(() =>
    bucketData().reduce((sum, b) => sum + b.total, 0),
  );

  // 有阅读记录的书按总时长降序 → 堆叠与配色顺序；颜色按 book_id 经
  // 金色角色轮分配，同一本书在任意区间/排序下颜色稳定
  const series = createMemo<ChartSeriesItem[]>(() => {
    const titles = new Map<number, string>();
    for (const row of daily()?.days ?? []) titles.set(row.book_id, row.title);
    const totals = perBookTotals(bucketData());
    return [...totals]
      .filter(([, secs]) => secs > 0)
      .toSorted((a, b) => b[1] - a[1])
      .map(([id]) => ({
        bookId: id,
        color: goldenColor(id),
        name: titles.get(id) ?? `#${id}`,
      }));
  });

  const [hover, setHover] = createSignal<ChartHover | null>(null);
  const [focusBookId, setFocusBookId] = createSignal<null | number>(null);

  // 区间切换后原悬停桶已不存在
  createEffect(() => {
    range();
    setHover(null);
  });

  // 列表作用域键（'all' 或桶 key）：悬停空列视为 'all'，跨空列移动不重建列表
  const listScope = createMemo(() => {
    const h = hover();
    if (!h) return 'all';
    const b = bucketData().find(x => x.key === h.bucketKey);
    return b && b.total > 0 ? b.key : 'all';
  });

  // 列表行：常态为整个区间；悬停有数据的列时仅显示该桶切片
  const rows = createMemo<BookBarRow[]>(() => {
    const titles = new Map<number, string>();
    for (const row of daily()?.days ?? []) titles.set(row.book_id, row.title);
    const scope = listScope();
    const source =
      scope === 'all'
        ? perBookTotals(bucketData())
        : (bucketData().find(b => b.key === scope)?.perBook ??
          new Map<number, number>());
    return [...source]
      .filter(([, secs]) => secs > 0)
      .toSorted((a, b) => b[1] - a[1])
      .map(([id, secs]) => ({
        bookId: id,
        color: goldenColor(id),
        name: titles.get(id) ?? `#${id}`,
        secs,
      }));
  });

  // 列表作用域切换时重放一次淡入（WAAPI 作用于包装层，行与色点不重挂载）
  let listRef: HTMLDivElement | undefined;
  createEffect(
    on(
      listScope,
      () => {
        listRef?.animate(
          [
            { opacity: 0, transform: 'translateY(3px)' },
            { opacity: 1, transform: 'translateY(0)' },
          ],
          { duration: 200, easing: 'ease-out' },
        );
      },
      { defer: true },
    ),
  );

  /** 跳到所选日期所在的周 / 月 / 年 */
  const jumpToDate = (value: string) => {
    const off = offsetForDate(granularity(), value, weekFirstDay(), todayKey());
    if (off == null) return;
    setOffset(Math.min(0, off));
    setPickerOpen(false);
  };

  const periodLabel = createMemo(() => {
    const keys = PERIOD_KEYS[granularity()];
    return t(offset() === 0 ? keys.current : keys.other);
  });

  const dailyError = createMemo(() => {
    // Solid 的 resource.error 类型为 any，显式收窄
    const err: unknown = daily.error;
    if (!err) return null;
    return err instanceof ApiError ? err.message : t('errorLoadFailed');
  });

  // 书不存在或 id 非法时仍可打开弹窗（由弹窗内 GET 报错兜底）
  const editingName = createMemo(() => {
    const id = editingBookId();
    if (id == null) return '';
    // 从已加载的区间明细取书名；当前区间无记录（如深链直开）时回退 #id
    const titles = new Map<number, string>();
    for (const row of daily()?.days ?? []) titles.set(row.book_id, row.title);
    return titles.get(id) ?? `#${id}`;
  });

  return (
    <div class="space-y-4">
      <h1 class="text-lg font-medium">{t('statsTitle')}</h1>

      <Show
        fallback={
          <StateBlock
            error={statsError()}
            onRetry={() => {
              void refetch();
            }}
          />
        }
        when={stats()}
      >
        {data => <SummaryCards stats={data()} />}
      </Show>

      {/* ── 时间控件 ── */}
      <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-2">
        <div class="flex items-center gap-1.5">
          <div class="border-edge flex rounded-lg border p-0.5">
            <For each={GRANULARITIES}>
              {g => (
                <button
                  class={`cursor-pointer rounded-md px-3 py-1 text-sm transition-colors ${
                    granularity() === g
                      ? 'bg-ink text-page shadow-sm'
                      : 'text-muted hover:bg-page'
                  }`}
                  onClick={() => {
                    setGranularity(g);
                    setOffset(0);
                  }}
                >
                  {t(
                    g === 'week'
                      ? 'statsGranularityWeek'
                      : g === 'month'
                        ? 'statsGranularityMonth'
                        : 'statsGranularityYear',
                  )}
                </button>
              )}
            </For>
          </div>
          <Show when={offset() !== 0}>
            <IconButton
              aria-label={t('statsBackToCurrent')}
              onClick={() => {
                setOffset(0);
              }}
            >
              <VsRefresh class="size-4" />
            </IconButton>
          </Show>
        </div>

        <div class="flex items-center gap-1">
          <IconButton
            aria-label={t('statsPrevPeriod')}
            onClick={() => {
              setOffset(o => o - 1);
            }}
          >
            <VsChevronLeft class="size-4" />
          </IconButton>
          <button
            class="text-muted hover:bg-page hover:text-ink cursor-pointer rounded-md px-2 py-1 text-sm font-medium tabular-nums"
            onClick={() => {
              setPickerOpen(true);
            }}
            title={t('statsJumpToDate')}
            type="button"
          >
            {rangeLabel(range())}
          </button>
          <IconButton
            aria-label={t('statsNextPeriod')}
            disabled={offset() === 0}
            onClick={() => {
              setOffset(o => Math.min(0, o + 1));
            }}
          >
            <VsChevronRight class="size-4" />
          </IconButton>
        </div>

        <span class="text-muted text-sm">
          {periodLabel()}
          {'：'}
          <span class="text-ink font-bold tabular-nums">
            {formatDuration(rangeTotal(), t)}
          </span>
        </span>
      </div>

      {/* ── 堆叠柱状图 ── */}
      <SectionCard>
        <Show
          fallback={
            <StateBlock
              error={dailyError()}
              onRetry={() => {
                void dailyActions.refetch();
              }}
            />
          }
          when={!daily.error}
        >
          <Show
            fallback={<EmptyState text={t('statsNoDataRange')} />}
            when={rangeTotal() > 0}
          >
            <div class="h-64 sm:h-80">
              <StackedChart
                data={bucketData()}
                focusBookId={focusBookId()}
                onHover={setHover}
                series={series()}
              />
            </div>
          </Show>
        </Show>
      </SectionCard>

      {/* ── 分书籍时长 ── */}
      <SectionCard title={t('statsPerBook')}>
        <div class="max-h-72 scrollbar-none overflow-y-auto" ref={listRef}>
          <BookPlaytimeBars
            emptyText={t('statsNoDataInScope')}
            highlightBookId={hover()?.bookId ?? null}
            onEditBook={setEditingBookId}
            onHoverBook={setFocusBookId}
            rows={rows()}
          />
        </div>
      </SectionCard>

      <Modal
        onClose={() => {
          setPickerOpen(false);
        }}
        open={pickerOpen()}
        title={t('statsJumpToDate')}
      >
        <input
          class="border-edge bg-surface text-ink focus:border-accent w-full rounded border px-3 py-1.5 text-sm outline-none"
          max={todayKey()}
          onChange={event => {
            jumpToDate(event.currentTarget.value);
          }}
          type="date"
          value={dateKey(range().start)}
        />
      </Modal>

      {/* keyed：把非空 id 以非空类型传给弹窗 */}
      <Show keyed when={editingBookId()}>
        {id => (
          <DailyEditModal
            bookId={id}
            color={goldenColor(id)}
            name={editingName()}
            today={todayKey()}
            onClose={() => {
              setEditingBookId(null);
            }}
            onSaved={() => {
              // 服务端已按差值同步总量，两个统计资源都要刷新
              void refetch();
              void dailyActions.refetch();
            }}
          />
        )}
      </Show>
    </div>
  );
};

const SummaryCards: Component<{ stats: ReadingStats }> = props => {
  const { t } = useI18n();
  return (
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-3">
      <StatCard
        icon={<VsWatch class="size-5" />}
        label={t('statsTotalTime')}
        value={formatDuration(props.stats.total_reading_seconds, t)}
      />
      <StatCard
        icon={<VsFlame class="size-5" />}
        label={t('statsCurrentStreak')}
        value={t('statsDays', { days: props.stats.current_streak_days })}
      />
      <StatCard
        icon={<VsTarget class="size-5" />}
        label={t('statsLongestStreak')}
        value={t('statsDays', { days: props.stats.longest_streak_days })}
      />
    </div>
  );
};

const StatCard: ParentComponent<{
  icon: JSX.Element;
  label: string;
  value: string;
}> = props => (
  <div class="border-edge bg-surface rounded-lg border p-4">
    <div class="text-muted flex items-center gap-2 text-sm">
      <span class="text-accent">{props.icon}</span>
      {props.label}
    </div>
    <div class="mt-2 text-2xl font-medium tabular-nums">{props.value}</div>
  </div>
);

const StateBlock: ParentComponent<{
  error: null | string;
  onRetry: () => void;
}> = props => {
  const { t } = useI18n();
  return (
    <div class="flex flex-col items-center gap-3 py-12">
      <Show fallback={<Spinner class="size-6" />} when={props.error}>
        <p class="text-muted text-sm">{props.error}</p>
        <Button onClick={props.onRetry} variant="ghost">
          {t('retry')}
        </Button>
      </Show>
    </div>
  );
};

export default Stats;
