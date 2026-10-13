import { describe, expect, it } from 'vitest';

import {
  aggregate,
  dateKey,
  localeFirstWeekday,
  offsetForDate,
  parseDateKey,
  resolveSelection,
  todayKeyIn,
} from './timeRange';

const UTC = (iso: string): Date => {
  const d = parseDateKey(iso);
  if (!d) throw new Error(`bad key ${iso}`);
  return d;
};

describe('dateKey / parseDateKey', () => {
  it('round-trips', () => {
    expect(dateKey(UTC('2026-10-14'))).toBe('2026-10-14');
  });

  it('rejects malformed keys', () => {
    expect(parseDateKey('2026-10')).toBeNull();
    expect(parseDateKey('oops')).toBeNull();
    expect(parseDateKey('2026-13-01')).toBeNull();
  });
});

describe('todayKeyIn', () => {
  it('resolves the calendar day of an IANA timezone', () => {
    // UTC+8 的 10 月 14 日 02:00，UTC 仍是 10 月 13 日
    const instant = Date.UTC(2026, 9, 13, 18, 0);
    const fmt = (tz: string): string => {
      const parts = new Intl.DateTimeFormat('en-US', {
        day: '2-digit',
        month: '2-digit',
        timeZone: tz,
        year: 'numeric',
      }).format(new Date(instant));
      // '10/14/2026' → 'YYYY-MM-DD'
      const [m, d, y] = parts.split('/');
      return `${y}-${m}-${d}`;
    };
    expect(fmt('Asia/Shanghai')).toBe(todayKeyIn('Asia/Shanghai'));
    expect(fmt('UTC')).toBe(todayKeyIn('UTC'));
  });
});

describe('localeFirstWeekday', () => {
  it('maps known locales', () => {
    // 周日开头（en-US）归一为 0；周一起始（zh-CN / ISO 默认）为 1
    expect(localeFirstWeekday('en-US')).toBe(0);
    expect(localeFirstWeekday('zh-CN')).toBe(1);
  });
});

describe('resolveSelection', () => {
  it('week: starts at locale first day and spans 7 days', () => {
    // 2026-10-14 是周三；firstDay=1（周一）→ 本周从 10-12 开始
    const r = resolveSelection(
      { granularity: 'week', offset: 0 },
      1,
      '2026-10-14',
    );
    expect(dateKey(r.start)).toBe('2026-10-12');
    expect(r.buckets).toHaveLength(7);
    expect(r.buckets[0]?.key).toBe('2026-10-12');
    expect(r.buckets[6]?.key).toBe('2026-10-18');
  });

  it('week: firstDay=0（周日）时从周日开始', () => {
    const r = resolveSelection(
      { granularity: 'week', offset: 0 },
      0,
      '2026-10-14',
    );
    expect(dateKey(r.start)).toBe('2026-10-11');
  });

  it('week: 负偏移回退整数周', () => {
    const r = resolveSelection(
      { granularity: 'week', offset: -1 },
      1,
      '2026-10-14',
    );
    expect(dateKey(r.start)).toBe('2026-10-05');
    expect(dateKey(r.end)).toBe('2026-10-12');
  });

  it('month: covers the full month incl. leap February', () => {
    const feb = resolveSelection(
      { granularity: 'month', offset: 0 },
      1,
      '2028-02-10',
    );
    expect(feb.buckets).toHaveLength(29);
    expect(feb.buckets[0]?.key).toBe('2028-02-01');
    expect(feb.buckets[28]?.key).toBe('2028-02-29');

    const prev = resolveSelection(
      { granularity: 'month', offset: -1 },
      1,
      '2028-03-05',
    );
    expect(prev.buckets).toHaveLength(29); // 2028-02
  });

  it('year: 12 month buckets keyed YYYY-MM', () => {
    const r = resolveSelection(
      { granularity: 'year', offset: 0 },
      1,
      '2026-10-14',
    );
    expect(r.buckets).toHaveLength(12);
    expect(r.buckets[0]?.key).toBe('2026-01');
    expect(r.buckets[11]?.key).toBe('2026-12');
    expect(r.buckets.every(b => b.unit === 'month')).toBe(true);
  });
});

describe('aggregate', () => {
  const rows = [
    { book_id: 1, date: '2026-10-12', reading_seconds: 60 },
    { book_id: 2, date: '2026-10-12', reading_seconds: 30 },
    { book_id: 1, date: '2026-10-13', reading_seconds: 120 },
    { book_id: 3, date: '2026-09-30', reading_seconds: 15 },
    { book_id: 1, date: '2026-10-14', reading_seconds: 0 }, // 零值行不进桶
  ];

  it('day buckets: per-book seconds and totals', () => {
    const r = resolveSelection(
      { granularity: 'week', offset: 0 },
      1,
      '2026-10-14',
    );
    const data = aggregate(rows, r.buckets);
    const mon = data[0]!;
    expect(mon.total).toBe(90);
    expect(mon.perBook.get(1)).toBe(60);
    expect(mon.perBook.get(2)).toBe(30);
    expect(data[1]!.perBook.get(1)).toBe(120);
    // 无记录的桶是空 map，仍占位
    expect(data[2]!.total).toBe(0);
  });

  it('month buckets: folds day rows by YYYY-MM', () => {
    const year = resolveSelection(
      { granularity: 'year', offset: 0 },
      1,
      '2026-10-14',
    );
    const data = aggregate(rows, year.buckets);
    const oct = data[9]!;
    expect(oct.total).toBe(210);
    expect(oct.perBook.get(1)).toBe(180);
    expect(oct.perBook.get(2)).toBe(30);
    const sep = data[8]!;
    expect(sep.total).toBe(15);
  });
});

describe('offsetForDate', () => {
  it('week offsets', () => {
    expect(offsetForDate('week', '2026-10-14', 1, '2026-10-14')).toBe(0);
    expect(offsetForDate('week', '2026-10-06', 1, '2026-10-14')).toBe(-1);
    expect(offsetForDate('week', '2026-11-20', 1, '2026-10-14')).toBe(5);
  });

  it('month / year offsets across year boundary', () => {
    expect(offsetForDate('month', '2026-01-20', 1, '2026-10-14')).toBe(-9);
    expect(offsetForDate('year', '2024-06-01', 1, '2026-10-14')).toBe(-2);
  });

  it('rejects malformed dates', () => {
    expect(offsetForDate('week', 'bad', 1, '2026-10-14')).toBeNull();
  });
});
