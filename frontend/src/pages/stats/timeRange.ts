// 统计图表的纯日期分桶与聚合逻辑，无 Solid / d3 / i18n 依赖，保持可单测。
//
// 服务端 /stats/daily 返回按用户时区归算的 'YYYY-MM-DD' 日期字符串；此处
// 一律把日期字符串按 UTC 午夜解析后只做日历日运算（无 DST、无时区换算），
// 与服务端日报的日期语义严格对齐。
//
// 图表消费平铺的 BucketDatum[]，因此接入新的区间来源（如自由日期范围）
// 只需产出对应的 Bucket[] 再调 aggregate，图表组件无需改动。

export interface Bucket {
  /** Exclusive bucket end. */
  end: Date;
  /** 'YYYY-MM-DD' for day buckets, 'YYYY-MM' for month buckets. */
  key: string;
  /** Inclusive bucket start (UTC midnight / first of month). */
  start: Date;
  unit: BucketUnit;
}

export interface BucketDatum extends Bucket {
  /** book id -> seconds read within this bucket (only non-zero entries). */
  perBook: Map<number, number>;
  total: number;
}

export type BucketUnit = 'day' | 'month';

/** 服务器日报行（/stats/daily 的 days 元素）所需的形状 */
export interface DayRow {
  book_id: number;
  date: string;
  reading_seconds: number;
}

export type Granularity = 'month' | 'week' | 'year';

export interface ResolvedRange {
  buckets: Bucket[];
  /** Exclusive range end. */
  end: Date;
  start: Date;
}

export interface TimeSelection {
  granularity: Granularity;
  /** 0 = current period, -1 = previous, +1 = next, … */
  offset: number;
}

const DAY_MS = 86_400_000;

export const pad2 = (n: number): string => String(n).padStart(2, '0');

/** UTC 午夜 Date → 'YYYY-MM-DD'，与后端 chrono 的 %Y-%m-%d 对齐 */
export const dateKey = (d: Date): string =>
  `${d.getUTCFullYear()}-${pad2(d.getUTCMonth() + 1)}-${pad2(d.getUTCDate())}`;

export const monthKey = (d: Date): string =>
  `${d.getUTCFullYear()}-${pad2(d.getUTCMonth() + 1)}`;

/** 'YYYY-MM-DD' → UTC 午夜 Date；格式非法或字段溢出（如 13 月）返回 null */
export function parseDateKey(key: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(key);
  if (!m) return null;
  const d = new Date(Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3])));
  // Date 会把溢出字段进位（2026-13-01 → 2027-01-01），回环比对拒绝之
  return Number.isNaN(d.getTime()) || dateKey(d) !== key ? null : d;
}

export const addDays = (d: Date, days: number): Date =>
  new Date(d.getTime() + days * DAY_MS);

/**
 * 把日报行聚合进分桶。日分桶按 date 直接命中；月分桶先把行折叠进
 * 'YYYY-MM' 键（O(rows)），再逐桶 O(1) 取用。
 */
export function aggregate(
  rows: readonly DayRow[],
  buckets: readonly Bucket[],
): BucketDatum[] {
  const allDays = buckets.every(b => b.unit === 'day');
  // 键 → book → 秒
  const folded = new Map<string, Map<number, number>>();
  for (const row of rows) {
    if (row.reading_seconds <= 0) continue;
    const key = allDays ? row.date : row.date.slice(0, 7);
    let perBook = folded.get(key);
    if (!perBook) {
      perBook = new Map();
      folded.set(key, perBook);
    }
    perBook.set(
      row.book_id,
      (perBook.get(row.book_id) ?? 0) + row.reading_seconds,
    );
  }
  return buckets.map(b => {
    const perBook = folded.get(b.key) ?? new Map<number, number>();
    let total = 0;
    for (const secs of perBook.values()) total += secs;
    return { ...b, perBook, total };
  });
}

export function buildDayBuckets(start: Date, count: number): Bucket[] {
  return Array.from({ length: count }, (_, index) => {
    const s = addDays(start, index);
    return {
      end: addDays(s, 1),
      key: dateKey(s),
      start: s,
      unit: 'day' as const,
    };
  });
}

/**
 * 给定区间（含端点）展开为日分桶；跨度上限由服务端（372 天）保证。
 */
export function buildDayBucketsForRange(
  startKey: string,
  endKey: string,
): Bucket[] | null {
  const start = parseDateKey(startKey);
  const end = parseDateKey(endKey);
  if (!start || !end || start > end) return null;
  const days = Math.round((end.getTime() - start.getTime()) / DAY_MS) + 1;
  return buildDayBuckets(start, days);
}

export function buildMonthBuckets(year: number, count = 12): Bucket[] {
  return Array.from({ length: count }, (_, index) => {
    const s = new Date(Date.UTC(year, index, 1));
    return {
      end: new Date(Date.UTC(year, index + 1, 1)),
      key: `${year}-${pad2(index + 1)}`,
      start: s,
      unit: 'month' as const,
    };
  });
}

/**
 * locale 的一周首日，归一到 0=周日…6=周六。使用平台的
 * `Intl.Locale.weekInfo`（Chromium 99+），失败回退周一（ISO-8601 默认）。
 */
export function localeFirstWeekday(locale: string): number {
  try {
    const info = (
      new Intl.Locale(locale) as Intl.Locale & {
        weekInfo?: { firstDay?: number };
      }
    ).weekInfo;
    // Intl 报周日为 7；归一到 0 基（0=周日）
    return (info?.firstDay ?? 1) % 7;
  } catch {
    return 1;
  }
}

/**
 * date 相对 today 所在周期（按粒度）的偏移，过去为负。用于日期跳转：
 * 任选一天 → 展示其所在周 / 月 / 年。
 */
export function offsetForDate(
  granularity: Granularity,
  dateKeyStr: string,
  weekFirstDay: number,
  todayKey: string,
): null | number {
  const date = parseDateKey(dateKeyStr);
  const today = parseDateKey(todayKey);
  if (!date || !today) return null;
  switch (granularity) {
    case 'month': {
      return (
        (date.getUTCFullYear() - today.getUTCFullYear()) * 12 +
        (date.getUTCMonth() - today.getUTCMonth())
      );
    }
    case 'week': {
      const a = startOfWeek(today, weekFirstDay);
      const b = startOfWeek(date, weekFirstDay);
      return Math.round((b.getTime() - a.getTime()) / (7 * DAY_MS));
    }
    case 'year': {
      return date.getUTCFullYear() - today.getUTCFullYear();
    }
  }
}

/** 区间内每本书的总秒数 */
export function perBookTotals(
  data: readonly BucketDatum[],
): Map<number, number> {
  const totals = new Map<number, number>();
  for (const b of data) {
    for (const [id, secs] of b.perBook) {
      totals.set(id, (totals.get(id) ?? 0) + secs);
    }
  }
  return totals;
}

/** 区间的 ISO 标签：'2026-10-05 - 2026-10-11'（end 为排他端点，展示时退一天） */
export function rangeLabel(range: ResolvedRange): string {
  return `${dateKey(range.start)} - ${dateKey(addDays(range.end, -1))}`;
}

/** 把周 / 月 / 年选择展开为具体分桶 */
export function resolveSelection(
  sel: TimeSelection,
  weekFirstDay: number,
  todayKey: string,
): ResolvedRange {
  const today = parseDateKey(todayKey);
  if (!today) throw new Error(`invalid todayKey: ${todayKey}`);
  switch (sel.granularity) {
    case 'month': {
      const start = new Date(
        Date.UTC(today.getUTCFullYear(), today.getUTCMonth() + sel.offset, 1),
      );
      const end = new Date(
        Date.UTC(start.getUTCFullYear(), start.getUTCMonth() + 1, 1),
      );
      const days = Math.round((end.getTime() - start.getTime()) / DAY_MS);
      return { buckets: buildDayBuckets(start, days), end, start };
    }
    case 'week': {
      const start = addDays(startOfWeek(today, weekFirstDay), sel.offset * 7);
      return {
        buckets: buildDayBuckets(start, 7),
        end: addDays(start, 7),
        start,
      };
    }
    case 'year': {
      const year = today.getUTCFullYear() + sel.offset;
      return {
        buckets: buildMonthBuckets(year),
        end: new Date(Date.UTC(year + 1, 0, 1)),
        start: new Date(Date.UTC(year, 0, 1)),
      };
    }
  }
}

/** 包含 d 的那一周的首日（本地日历语义，d 为 UTC 午夜） */
export function startOfWeek(d: Date, firstDay: number): Date {
  const diff = (d.getUTCDay() - firstDay + 7) % 7;
  return addDays(d, -diff);
}

/** 'now'（用户时区语义的今天）在指定 IANA 时区的 'YYYY-MM-DD' */
export function todayKeyIn(tz: string): string {
  // formatToParts 逐段取值，避免依赖 locale 的字段顺序
  const parts = new Intl.DateTimeFormat('en-US', {
    day: '2-digit',
    month: '2-digit',
    timeZone: tz,
    year: 'numeric',
  }).formatToParts(new Date());
  const get = (type: Intl.DateTimeFormatPartTypes): string =>
    parts.find(p => p.type === type)?.value ?? '01';
  return `${get('year')}-${get('month')}-${get('day')}`;
}
