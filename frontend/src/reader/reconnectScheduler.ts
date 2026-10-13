// WS 重连调度状态机：退避节奏、在途哨兵与放弃状态。纯逻辑、无副作用，
// 便于单测；定时器句柄由调用方持有，这里只跟踪"是否有重连在途"。

/**
 * 重连调度器。在途哨兵（timerPending）在定时器触发后必须复位（settle），
 * 否则手动重连 / 网络恢复重试会被永久拦截。
 */
export class ReconnectScheduler {
  private attempts = 0;
  private backoffMs: number;
  private gaveUp = false;
  private readonly initialBackoffMs: number;
  private timerPending = false;

  constructor(
    initialBackoffMs: number,
    private readonly maxAttempts: number,
    private readonly maxBackoffMs: number,
  ) {
    this.backoffMs = initialBackoffMs;
    this.initialBackoffMs = initialBackoffMs;
  }

  /** 自动重连已放弃：等待手动重连或网络恢复 */
  isExhausted(): boolean {
    return this.gaveUp;
  }

  /** 无定时重连在途：允许发起新连接（手动重连的唯一拦截条件） */
  isIdle(): boolean {
    return !this.timerPending;
  }

  /** 排下一次定时重连，返回退避间隔；超过上限置放弃态并返回 null */
  next(): null | number {
    this.attempts += 1;
    if (this.attempts > this.maxAttempts) {
      this.gaveUp = true;
      this.timerPending = false;
      return null;
    }
    this.timerPending = true;
    const delay = this.backoffMs;
    this.backoffMs = Math.min(this.backoffMs * 2, this.maxBackoffMs);
    return delay;
  }

  /** 连接成功或手动重连：退避与放弃状态全部归零 */
  reset(): void {
    this.attempts = 0;
    this.backoffMs = this.initialBackoffMs;
    this.gaveUp = false;
    this.timerPending = false;
  }

  /** 定时重连已触发或被取消：复位在途哨兵 */
  settle(): void {
    this.timerPending = false;
  }
}
