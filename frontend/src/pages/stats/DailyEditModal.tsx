// 编辑某本书每日阅读时长的弹窗：[日期] [时] [分] [删除] 行（最新在前）
// + 底部“添加日期”选择器。逻辑移植自 GalgameManager 的
// DailyPlaytimeEditModal，封面缩略图改为色点（本项目无封面接口）。
//
// 打开时 GET 该书每日记录作为快照；本地草稿为快照的浅拷贝；保存前重新
// GET live 并做三方合并（dailyEdit.ts），弹窗打开期间的心跳累加不丢，
// 随后全量 PUT，由服务端按差值同步累计时长与用户总时长。
import { VsTrash } from 'solid-icons/vs';
import {
  createMemo,
  createSignal,
  For,
  onMount,
  Show,
  type Component,
} from 'solid-js';
import { createStore, produce, unwrap } from 'solid-js/store';
import toast from 'solid-toast';

import { readingApi } from '../../api';
import { Button, Input, Modal, Spinner } from '../../components/ui';
import { useI18n } from '../../i18n';
import { formatDuration } from '../../utils/format';
import {
  MAX_DAILY_SECONDS,
  mergeDailySeconds,
  setHourMinute,
  sumDaily,
  toHourMinute,
  type DailySecondsMap,
} from './dailyEdit';

interface DailyEditModalProps {
  /** 被编辑的书 id */
  bookId: number;
  /** 书名旁的色点颜色（统计页金色角色轮） */
  color: string;
  /** 书名（当前区间无记录的深链场景下调用方回退为 #id） */
  name: string;
  /** 今天（用户时区 YYYY-MM-DD）：新增日期选择器的上限 */
  today: string;
  /** 关闭（不保存） */
  onClose: () => void;
  /** 保存成功后由父级刷新统计资源（stats 与 daily） */
  onSaved: () => void;
}

const DailyEditModal: Component<DailyEditModalProps> = props => {
  const { t } = useI18n();

  // 快照：打开时的服务端记录（三方合并基线）；null = 尚未载入
  const [snapshot, setSnapshot] = createSignal<DailySecondsMap | null>(null);
  const [loadError, setLoadError] = createSignal(false);
  const [busy, setBusy] = createSignal(false);
  // 浅拷贝即可：值是 number；不能直接引用快照，否则编辑会污染合并基线
  const [draft, setDraft] = createStore<DailySecondsMap>({});

  onMount(() => {
    void (async () => {
      try {
        const data = await readingApi.bookDailyStats(props.bookId);
        const map: DailySecondsMap = {};
        for (const entry of data.entries)
          map[entry.date] = entry.reading_seconds;
        setSnapshot(map);
        setDraft(map);
      } catch {
        setLoadError(true);
      }
    })();
  });

  // 最新在前：补记“今天”是最常见操作
  const sortedDates = createMemo(() =>
    Object.keys(draft).toSorted().toReversed(),
  );

  const draftTotal = createMemo(() => sumDaily(draft));
  /** 保存将对累计时长产生的调整量预览（以打开时快照为基线） */
  const deltaText = createMemo(() => {
    const snap = snapshot();
    if (!snap) return '';
    const delta = draftTotal() - sumDaily(snap);
    if (delta === 0) return '';
    return `${delta > 0 ? '+' : '-'}${formatDuration(Math.abs(delta), t)}`;
  });

  // 单日条目上限校验（与服务端 MAX_READING_TIME_SECS 对齐）：
  // 时输入无 max、分手输可超 59，超限载荷会被服务端 400 且只回通用文案，
  // 这里提前给出原因并禁用保存（与“修改阅读时长”弹窗同款体验）
  const entriesValid = createMemo(() =>
    Object.values(draft).every(secs => secs <= MAX_DAILY_SECONDS),
  );

  const setHM = (date: string, h: number, m: number) => {
    setDraft(date, setHourMinute(draft[date] ?? 0, h, m));
  };

  const [newDate, setNewDate] = createSignal('');

  // 行以 data-date 标记：新增（或已存在）行可在排序插入后被定位，
  // 与 BookPlaytimeBars 的联动滚动同一套 querySelector 方案
  let listRef: HTMLDivElement | undefined;

  /** 已有行提示：滚入视野并淡出高亮环 */
  const flashRow = (date: string) => {
    const el = listRef?.querySelector<HTMLElement>(
      `[data-date="${CSS.escape(date)}"]`,
    );
    if (!el) return;
    // nearest：行已可见时是 no-op，只滚动最近的滚动祖先
    el.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    el.animate(
      [
        { boxShadow: '0 0 0 2px var(--color-accent)' },
        { boxShadow: '0 0 0 2px transparent' },
      ],
      { duration: 1500, easing: 'ease-out' },
    );
  };

  const handleAddDate = (value: string) => {
    if (!value) return;
    if (draft[value] !== undefined) {
      // solid-toast 无 warning 变体，用 error 作为提示色
      toast.error(t('statsDateExists'));
      flashRow(value);
      return;
    }
    setDraft(value, 0);
    setNewDate('');
    flashRow(value);
  };

  const save = async () => {
    const snap = snapshot();
    if (!snap) return;
    setBusy(true);
    try {
      // live：保存瞬间的服务端记录，合并保留打开期间的心跳累加
      const live = await readingApi.bookDailyStats(props.bookId);
      const liveMap: DailySecondsMap = {};
      for (const entry of live.entries)
        liveMap[entry.date] = entry.reading_seconds;
      const merged = mergeDailySeconds(snap, unwrap(draft), liveMap);
      await readingApi.updateBookDailyStats(
        props.bookId,
        Object.entries(merged).map(([date, reading_seconds]) => ({
          date,
          reading_seconds,
        })),
      );
      toast.success(t('statsDailySaved'));
      props.onSaved();
      props.onClose();
    } catch {
      toast.error(t('errorGeneric'));
    } finally {
      setBusy(false);
    }
  };

  /** 行内的时/分输入（秒位由 setHourMinute 保留） */
  const hmField = (
    date: string,
    key: 'h' | 'm',
    value: () => number,
    max: number | undefined,
    label: string,
  ) => (
    <label class="min-w-0 flex-1">
      <span class="text-muted sr-only">{label}</span>
      <Input
        max={max}
        min={0}
        onInput={v => {
          const hm = toHourMinute(draft[date] ?? 0);
          const parsed = Number.parseInt(v, 10);
          const n = Number.isNaN(parsed) ? 0 : parsed;
          setHM(date, key === 'h' ? n : hm.h, key === 'm' ? n : hm.m);
        }}
        size="sm"
        type="number"
        value={String(value())}
      />
    </label>
  );

  return (
    <Modal onClose={props.onClose} open title={t('editDailyStats')}>
      {/* 头部：色点 + 书名 + 差值预览 */}
      <div class="mb-3 flex items-center gap-2">
        <span
          class="size-2.5 shrink-0 rounded-full"
          style={{ 'background-color': props.color }}
        />
        <span
          class="min-w-0 flex-1 truncate text-sm font-medium"
          title={props.name}
        >
          {props.name}
        </span>
      </div>

      <Show
        fallback={
          <p class="text-muted py-6 text-center text-sm">
            {t('errorLoadFailed')}
          </p>
        }
        when={!loadError()}
      >
        <Show
          fallback={
            <div class="flex justify-center py-6">
              <Spinner class="size-6" />
            </div>
          }
          when={snapshot() !== null}
        >
          {/* 行列表。p-1：overflow-y-auto 各轴都会裁剪，闪烁环需要四周留白 */}
          <div
            class="max-h-72 scrollbar-none overflow-y-auto p-1"
            ref={listRef}
          >
            <div class="flex flex-col gap-0.5">
              <For
                each={sortedDates()}
                fallback={
                  <div class="text-muted py-6 text-center text-sm">
                    {t('statsEmptyDaily')}
                  </div>
                }
              >
                {date => {
                  const hm = createMemo(() => toHourMinute(draft[date] ?? 0));
                  return (
                    <div
                      class="hover:bg-page grid grid-cols-[5.5rem_1fr_1fr_auto] items-center gap-2 rounded-md px-1 py-1"
                      data-date={date}
                    >
                      <span class="text-muted shrink-0 text-xs tabular-nums">
                        {date}
                      </span>
                      {hmField(
                        date,
                        'h',
                        () => hm().h,
                        undefined,
                        t('durationHours'),
                      )}
                      {hmField(
                        date,
                        'm',
                        () => hm().m,
                        59,
                        t('durationMinutes'),
                      )}
                      <button
                        aria-label={t('statsRemoveDate')}
                        class="text-muted shrink-0 cursor-pointer rounded p-1.5 transition-colors hover:text-red-600 dark:hover:text-red-400"
                        onClick={() => {
                          // 类型层面 Record<string, number> 不接受 undefined，
                          // 删键需经 produce
                          setDraft(produce(d => delete d[date]));
                        }}
                        title={t('statsRemoveDate')}
                        type="button"
                      >
                        <VsTrash class="size-3.5" />
                      </button>
                    </div>
                  );
                }}
              </For>
            </div>
          </div>

          {/* 添加日期 */}
          <div class="border-edge mt-3 flex items-center gap-2 border-t pt-3">
            <input
              class="border-edge bg-surface text-ink focus:border-accent w-full rounded border px-2 py-1 text-sm tabular-nums outline-none"
              max={props.today}
              onChange={e => {
                setNewDate(e.currentTarget.value);
                handleAddDate(e.currentTarget.value);
              }}
              type="date"
              value={newDate()}
            />
          </div>

          {/* 底部：合计与差值预览 + 操作 */}
          <div class="border-edge mt-3 flex items-center justify-between gap-3 border-t pt-3">
            <div class="text-muted flex min-w-0 flex-col text-xs tabular-nums">
              <span>
                {t('statsDailyTotal')}: {formatDuration(draftTotal(), t)}
              </span>
              <Show when={deltaText()}>
                <span>
                  {t('statsDailySaveAdjusts')} ({deltaText()})
                </span>
              </Show>
              <Show when={!entriesValid()}>
                <span class="text-red-600 dark:text-red-400">
                  {t('statsDailyInvalid')}
                </span>
              </Show>
            </div>
            <div class="flex shrink-0 gap-2">
              <Button disabled={busy()} onClick={props.onClose} variant="ghost">
                {t('cancel')}
              </Button>
              <Button
                disabled={busy() || !entriesValid()}
                onClick={() => void save()}
              >
                {t('save')}
              </Button>
            </div>
          </div>
        </Show>
      </Show>
    </Modal>
  );
};

export default DailyEditModal;
