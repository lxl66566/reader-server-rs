import { describe, expect, it } from 'vitest';

import { isTap, TAP_MAX_MOVE_PX, TAP_MAX_MS, tapZone } from './gestures';

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
