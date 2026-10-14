import { describe, expect, it } from 'vitest';

import {
  mergeDailySeconds,
  setHourMinute,
  sumDaily,
  toHourMinute,
} from './dailyEdit';

describe('setHourMinute', () => {
  it('preserves the seconds part when h/m change', () => {
    // 3675s = 1h 1m 15s → 2h 3m 保留 15s
    expect(setHourMinute(3675, 2, 3)).toBe(2 * 3600 + 3 * 60 + 15);
  });

  it('clamps negative input to zero', () => {
    expect(setHourMinute(0, -1, 0)).toBe(0);
    // -5m + 40s 合计为负 → 整体钳制，不保留部分秒位
    expect(setHourMinute(100, 0, -5)).toBe(0);
  });
});

describe('toHourMinute', () => {
  it('splits whole minutes, floors the rest', () => {
    expect(toHourMinute(3675)).toEqual({ h: 1, m: 1 }); // 15s 余数丢弃
    expect(toHourMinute(0)).toEqual({ h: 0, m: 0 });
    expect(toHourMinute(7200)).toEqual({ h: 2, m: 0 });
  });
});

describe('sumDaily', () => {
  it('sums values and tolerates undefined', () => {
    expect(sumDaily({ a: 10, b: 32 })).toBe(42);
    expect(sumDaily(undefined)).toBe(0);
    expect(sumDaily({})).toBe(0);
  });
});

describe('mergeDailySeconds', () => {
  it('keeps the live value for keys the user did not touch', () => {
    // 弹窗打开期间，服务端心跳对 07-01 的后台累加
    const snapshot = { '2026-07-01': 60 };
    const draft = { '2026-07-01': 60 };
    const live = { '2026-07-01': 120 };
    expect(mergeDailySeconds(snapshot, draft, live)).toEqual({
      '2026-07-01': 120,
    });
  });

  it('user edits win over background writes on the same key', () => {
    const snapshot = { '2026-07-01': 60 };
    const draft = { '2026-07-01': 3600 };
    const live = { '2026-07-01': 120 };
    expect(mergeDailySeconds(snapshot, draft, live)).toEqual({
      '2026-07-01': 3600,
    });
  });

  it('an edit reverted to the snapshot value falls back to live', () => {
    // draft == snapshot 表示净改动为零，live 值存活
    const snapshot = { '2026-07-01': 60 };
    const draft = { '2026-07-01': 60 };
    const live = { '2026-07-01': 120 };
    expect(mergeDailySeconds(snapshot, draft, live)).toEqual({
      '2026-07-01': 120,
    });
  });

  it('drops keys the user deleted', () => {
    const snapshot = { '2026-07-01': 60, '2026-07-02': 30 };
    const draft = { '2026-07-01': 60 };
    const live = { '2026-07-01': 60, '2026-07-02': 30 };
    expect(mergeDailySeconds(snapshot, draft, live)).toEqual({
      '2026-07-01': 60,
    });
  });

  it('keeps keys the server added while the modal was open', () => {
    const snapshot = { '2026-07-01': 60 };
    const draft = { '2026-07-01': 60 };
    const live = { '2026-07-01': 60, '2026-07-02': 90 };
    expect(mergeDailySeconds(snapshot, draft, live)).toEqual({
      '2026-07-01': 60,
      '2026-07-02': 90,
    });
  });

  it('keeps dates the user added', () => {
    const snapshot: Record<string, number> = {};
    const draft = { '2026-07-03': 1800 };
    const live: Record<string, number> = {};
    expect(mergeDailySeconds(snapshot, draft, live)).toEqual({
      '2026-07-03': 1800,
    });
  });

  it('drops zero-value entries (user zeroed a day)', () => {
    const snapshot = { '2026-07-01': 60, '2026-07-02': 30 };
    const draft = { '2026-07-01': 0, '2026-07-02': 30 };
    const live = { '2026-07-01': 60, '2026-07-02': 30 };
    expect(mergeDailySeconds(snapshot, draft, live)).toEqual({
      '2026-07-02': 30,
    });
  });
});
