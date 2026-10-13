// 统计图表的系列配色：金色角 HSL 色轮。按 book_id 旋转色相，
// 相邻 id 落在充分分离的色相上，且同一本书在任何区间/排序下颜色稳定。

const GOLDEN_ANGLE = 137.508;

export function goldenColor(seed: number): string {
  const hue = (seed * GOLDEN_ANGLE) % 360;
  return `hsl(${hue.toFixed(1)}, 65%, 55%)`;
}
