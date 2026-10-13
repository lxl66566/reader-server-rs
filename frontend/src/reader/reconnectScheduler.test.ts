import { describe, expect, it } from 'vitest';

import { ReconnectScheduler } from './reconnectScheduler';

describe('ReconnectScheduler', () => {
  it('按指数退避排程并在上限处封顶', () => {
    const s = new ReconnectScheduler(500, 10, 4000);
    expect(s.next()).toBe(500);
    expect(s.next()).toBe(1000);
    expect(s.next()).toBe(2000);
    expect(s.next()).toBe(4000);
    expect(s.next()).toBe(4000);
  });

  it('超过重连上限后置放弃态', () => {
    const s = new ReconnectScheduler(500, 2, 10_000);
    expect(s.next()).toBe(500);
    expect(s.next()).toBe(1000);
    expect(s.next()).toBeNull();
    expect(s.isExhausted()).toBe(true);
  });

  it('定时重连触发后必须 settle 才能再次手动重连（回归：哨兵不复位）', () => {
    const s = new ReconnectScheduler(500, 10, 10_000);
    s.next();
    expect(s.isIdle()).toBe(false);
    // timer 触发 / 被取消
    s.settle();
    expect(s.isIdle()).toBe(true);
  });

  it('放弃后手动重连 reset 恢复初始退避与可用状态', () => {
    const s = new ReconnectScheduler(500, 1, 10_000);
    s.next();
    expect(s.next()).toBeNull();
    expect(s.isExhausted()).toBe(true);
    s.reset();
    expect(s.isExhausted()).toBe(false);
    expect(s.isIdle()).toBe(true);
    expect(s.next()).toBe(500);
  });
});
