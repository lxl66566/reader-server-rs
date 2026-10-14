// 每日阅读时长编辑弹窗（DailyEditModal.tsx）的纯合并/换算逻辑，
// 无 Solid / i18n 依赖，保持可单测（与 timeRange.ts 同思路）。
//
// 服务端为权威数据源：保存时重新拉取最新记录做三方合并，弹窗打开
// 期间的心跳累加（“今天”）不会丢。

export type DailySecondsMap = Record<string, number>;
// 与服务端 MAX_READING_TIME_SECS 一致：单日条目上限（3650 天）
export const MAX_DAILY_SECONDS = 3650 * 24 * 60 * 60;
/**
 * 保存时把用户草稿合并进服务端最新状态（live）。
 *
 * - `snapshot`：弹窗打开时的每日记录；
 * - `draft`：用户编辑后的快照（新增键存在、删除键缺席、改动键已改值）；
 * - `live`：保存瞬间的服务端记录——弹窗打开期间心跳可能已累加“今天”。
 *
 * 逐键规则：
 * - draft 与 live 都有：用户改过（draft ≠ snapshot）取 draft，未改取 live
 *   （保留后台累加）；
 * - 仅 live 有：快照中没有（打开期间服务端新增）→ 保留；快照中有
 *   （用户删除）→ 丢弃；
 * - 仅 draft 有：用户新增的日期。
 * - 0 值条目丢弃：图表只统计 > 0，服务端也不落库 0。
 */
export function mergeDailySeconds(
  snapshot: DailySecondsMap,
  draft: DailySecondsMap,
  live: DailySecondsMap,
): DailySecondsMap {
  const merged: DailySecondsMap = {};
  const keys = new Set([...Object.keys(live), ...Object.keys(draft)]);
  for (const key of keys) {
    let secs: number | undefined;
    if (Object.hasOwn(draft, key) && Object.hasOwn(live, key)) {
      secs = draft[key] !== snapshot[key] ? draft[key] : live[key];
    } else if (Object.hasOwn(live, key)) {
      if (!Object.hasOwn(snapshot, key)) secs = live[key];
    } else {
      secs = draft[key];
    }
    if (secs !== undefined && secs > 0) merged[key] = secs;
  }
  return merged;
}

/**
 * 覆盖某日条目的时/分部分、保留秒位：与“修改阅读时长”弹窗对齐，
 * 心跳记录的秒级精度不会被分粒度编辑抹掉。
 */
export function setHourMinute(secs: number, h: number, m: number): number {
  // 钳制：min="0" 只约束步进器，手输 "-" 仍是负数，负值会被服务端 400
  return Math.max(0, h * 3600 + m * 60 + (secs % 60));
}

export function sumDaily(map: DailySecondsMap | undefined): number {
  let sum = 0;
  for (const v of Object.values(map ?? {})) sum += v;
  return sum;
}

/** 把某日条目的秒数拆为 {h, m}，供两个数字输入框使用 */
export function toHourMinute(secs: number): { h: number; m: number } {
  return { h: Math.floor(secs / 3600), m: Math.floor((secs % 3600) / 60) };
}
