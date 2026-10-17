import { presetWind4, type Theme } from '@unocss/preset-wind4';
import { theme as wind4Theme } from '@unocss/preset-wind4/theme';
import { defineConfig, type UserConfig } from 'unocss';

// Theme-aware colors map to CSS variables defined in index.css,
// so light/sepia/dark switching needs no class regeneration.
const config: UserConfig<Theme> = defineConfig({
  // 原生 select 无 pointer 光标，全局补齐
  preflights: [
    {
      getCSS: () => 'select, option { cursor: pointer; }',
      layer: 'base',
    },
  ],
  // 应用主题挂在 <html data-theme> 上，dark: 变体跟随它而非系统 prefers-color-scheme
  presets: [presetWind4({ dark: { dark: ':root[data-theme="dark"]' } })],
  postprocess: [
    util => {
      const fallbacks: [string, string][] = [];
      for (const [prop, value] of util.entries) {
        if (typeof value !== 'string') continue;
        const fallback = colorMixFallback(value);
        if (fallback !== null) fallbacks.push([prop, fallback]);
      }
      // 回退声明必须排在 color-mix 之前：新内核后者覆盖前者；老内核
      // （color-mix 需 Chromium 111+ / Safari 16.2+）整条声明失效只剩
      // 回退值，否则所有背景/边框透明
      util.entries.unshift(...fallbacks);
    },
  ],
  theme: {
    colors: {
      accent: 'var(--color-accent)',
      edge: 'var(--color-border)',
      highlight: 'var(--color-highlight)',
      ink: 'var(--color-fg)',
      muted: 'var(--color-muted)',
      // named "page" not "base": a color named "base" makes the Tailwind
      // `text-base` utility resolve to this color instead of font-size
      page: 'var(--color-bg)',
      paper: 'var(--color-paper)',
      surface: 'var(--color-surface)',
    },
  },
});

export default config;

// color-mix(in srgb, VAR [P%|var(--un-x-opacity)], transparent) → 静态可求值
// 的回退颜色。项目自定义色（--color-*）构建期取不到值，回退为不透明变量；
// wind4 内置字面色（--colors-*）按透明度合成 rgba，缺省透明度为 1。
// dev 模式下 wind4 会在值尾附加原始色注释，匹配前先剥掉
const COLOR_MIX_RE =
  /^color-mix\(in srgb, var\(--(colors?)-([a-z-]+)\)(?: (?:([\d.]+)%|var\(--un-[a-z-]+\)))?, transparent\)$/;

function colorMixFallback(rawValue: string): string | null {
  const value = rawValue.replace(/ \/\* .* \*\/$/, '');
  const match = COLOR_MIX_RE.exec(value);
  if (!match) return null;
  const [scope, name, percent] = [match[1], match[2], match[3]];
  if (scope === undefined || name === undefined) return null;
  if (scope === 'color') return `var(--color-${name})`;
  const hex = (wind4Theme.colors as Record<string, unknown>)[name];
  if (typeof hex !== 'string' || !/^#[0-9a-f]+$/i.test(hex)) return null;
  const n = Number.parseInt(hex.slice(1), 16);
  // 逗号 rgba：目标老内核对空格斜杠的现代写法支持不全
  const alpha = percent === undefined ? 1 : Number.parseFloat(percent) / 100;
  return `rgba(${(n >> 16) & 0xff}, ${(n >> 8) & 0xff}, ${n & 0xff}, ${alpha})`;
}
