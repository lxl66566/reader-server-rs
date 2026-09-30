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
  it('splits the width into left / center / right thirds', () => {
    expect(tapZone(0, 300)).toBe('left');
    expect(tapZone(98, 300)).toBe('left');
    expect(tapZone(100, 300)).toBe('center');
    expect(tapZone(200, 300)).toBe('center');
    expect(tapZone(202, 300)).toBe('right');
    expect(tapZone(299, 300)).toBe('right');
  });

  it('clamps a zero width instead of dividing by zero', () => {
    expect(tapZone(0.5, 0)).toBe('center');
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
