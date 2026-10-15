// 统计图表的系列配色：金色角色轮。按 book_id 旋转色相，相邻 id 落在
// 充分分离的色相上，且同一本书在任何区间/排序下颜色稳定。
// 明度/彩度随主题解析结果取值（oklch 保证各色相明度均匀）：暗色主题
// 降彩度提明度成粉彩，避免高饱和纯色在深底上刺眼；浅色/羊皮纸主题
// 加深明度保证可读性。
import type { ThemeMode } from '../../api/types';

const GOLDEN_ANGLE = 137.508;

type ResolvedTheme = Exclude<ThemeMode, 'system'>;

// oklch [lightness, chroma]，按主题目测调校
const TONE: Record<ResolvedTheme, { c: number; l: number }> = {
  dark: { c: 0.1, l: 0.75 },
  light: { c: 0.12, l: 0.55 },
  sepia: { c: 0.11, l: 0.52 },
};

export function goldenColor(
  seed: number,
  resolvedTheme: ResolvedTheme,
): string {
  const hue = (seed * GOLDEN_ANGLE) % 360;
  const { c, l } = TONE[resolvedTheme];
  return `oklch(${l} ${c} ${hue.toFixed(1)})`;
}
