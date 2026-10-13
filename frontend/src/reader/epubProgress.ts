// EPUB 进度降级换算：locations 生成慢/失败时按 spine 下标估算 percent，
// 保证心跳与进度保存不被大书卡死。

/** locations 生成的超时上限：超时后降级为 spine 估算 */
export const LOCATIONS_TIMEOUT_MS = 15_000;

/**
 * locations 未就绪时的估算 percent：按 spine 下标取章节中点。
 * 精度退化到章级，但结果恒在 (0, 1) 且随阅读单调推进，心跳得以继续。
 */
export function estimatePercent(
  spineIndex: number,
  spineLength: number,
): number {
  if (spineLength <= 0) return 0;
  return Math.min(1, Math.max(0, (spineIndex + 0.5) / spineLength));
}

/**
 * promise 在限时内 settle 则返回其成败，超时返回 false。
 * 原 promise 的拒绝始终被链上捕获，超时后不会变成 unhandled rejection。
 */
export async function settlesWithin(
  promise: Promise<unknown>,
  ms: number,
): Promise<boolean> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise.then(
        () => true,
        () => false,
      ),
      new Promise<boolean>(resolve => {
        timer = setTimeout(() => {
          resolve(false);
        }, ms);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
