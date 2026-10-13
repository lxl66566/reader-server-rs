import { describe, expect, it } from 'vitest';

import { estimatePercent, settlesWithin } from './epubProgress';

describe('estimatePercent', () => {
  it('maps spine index to the chapter midpoint', () => {
    expect(estimatePercent(0, 10)).toBe(0.05);
    expect(estimatePercent(4, 10)).toBe(0.45);
    expect(estimatePercent(9, 10)).toBe(0.95);
  });

  it('stays strictly inside (0, 1) and advances monotonically', () => {
    let prev = 0;
    for (let i = 0; i < 12; i++) {
      const percent = estimatePercent(i, 12);
      expect(percent).toBeGreaterThan(prev);
      expect(percent).toBeLessThan(1);
      prev = percent;
    }
    // 单章书也保证非零，避免被当作"未开始"
    expect(estimatePercent(0, 1)).toBe(0.5);
  });

  it('handles a degenerate spine', () => {
    expect(estimatePercent(3, 0)).toBe(0);
  });
});

describe('settlesWithin', () => {
  it('reports the outcome of a promise that settles in time', async () => {
    await expect(settlesWithin(Promise.resolve('x'), 1000)).resolves.toBe(true);
    await expect(
      settlesWithin(Promise.reject(new Error('boom')), 1000),
    ).resolves.toBe(false);
  });

  it('resolves false on timeout', async () => {
    const start = Date.now();
    await expect(settlesWithin(new Promise(() => undefined), 30)).resolves.toBe(
      false,
    );
    expect(Date.now() - start).toBeGreaterThanOrEqual(25);
  });

  it('does not leak the late rejection after timeout', async () => {
    const late = new Promise<never>((_, reject) => {
      setTimeout(() => {
        reject(new Error('late'));
      }, 50);
    });
    await expect(settlesWithin(late, 10)).resolves.toBe(false);
    // 等迟到的拒绝发生：已被链上捕获，不应出现 unhandled rejection
    await new Promise(resolve => setTimeout(resolve, 100));
  });
});
