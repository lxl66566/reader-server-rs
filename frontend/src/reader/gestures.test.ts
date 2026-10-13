import { describe, expect, it } from 'vitest';

import {
  isTap,
  TAP_MAX_MOVE_PX,
  TAP_MAX_MS,
  tapZone,
  tapZoneVertical,
} from './gestures';

describe('isTap', () => {
  it('accepts a quick, stationary press', () => {
    expect(isTap(TAP_MAX_MS - 1, 0, 0)).toBe(true);
    expect(isTap(100, TAP_MAX_MOVE_PX - 1, 0)).toBe(true);
  });

  it('rejects long presses', () => {
    expect(isTap(TAP_MAX_MS, 0, 0)).toBe(false);
    expect(isTap(TAP_MAX_MS + 1, 0, 0)).toBe(false);
  });

  it('rejects movement at or beyond the distance threshold', () => {
    expect(isTap(100, TAP_MAX_MOVE_PX, 0)).toBe(false);
    // 斜向位移按欧氏距离计算
    expect(isTap(100, TAP_MAX_MOVE_PX - 1, TAP_MAX_MOVE_PX - 1)).toBe(false);
  });
});

describe('tapZone', () => {
  const bounds = { left: 100, right: 400 };

  it('splits the container width into left / center / right thirds', () => {
    // 300px 宽的三区边界在 199 与 201（相对容器左缘 99 与 101）
    expect(tapZone(100, bounds)).toBe('left');
    expect(tapZone(198, bounds)).toBe('left');
    expect(tapZone(200, bounds)).toBe('center');
    expect(tapZone(300, bounds)).toBe('center');
    expect(tapZone(302, bounds)).toBe('right');
    expect(tapZone(399, bounds)).toBe('right');
  });

  it('subtracts the container offset for centered layouts', () => {
    // 回归：屏幕正中在居中容器内属于中央区，不能按 clientX/宽 误判为右区
    expect(tapZone(640, { left: 280, right: 1000 })).toBe('center');
    expect(tapZone(950, { left: 280, right: 1000 })).toBe('right');
  });

  it('treats a degenerate range as the center zone', () => {
    expect(tapZone(640, { left: 0, right: 0 })).toBe('center');
    expect(tapZone(640)).toBe('center');
  });
});

describe('tapZoneVertical', () => {
  const range = { bottom: 800, top: 200 };

  it('splits the visible range into top / center / bottom thirds', () => {
    // 600px 高的三区边界在 398 与 602（边界值本身归中区）
    expect(tapZoneVertical(200, range)).toBe('top');
    expect(tapZoneVertical(397, range)).toBe('top');
    expect(tapZoneVertical(398, range)).toBe('center');
    expect(tapZoneVertical(601, range)).toBe('center');
    expect(tapZoneVertical(603, range)).toBe('bottom');
    expect(tapZoneVertical(799, range)).toBe('bottom');
  });

  it('maps an offset coordinate space onto the visible range', () => {
    // epub iframe 的 clientY 相对整章内容，可视区是其中滚动的切片
    expect(tapZoneVertical(30, range)).toBe('top');
    expect(tapZoneVertical(900, range)).toBe('bottom');
  });
});
