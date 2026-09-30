import { presetWind4, type Theme } from '@unocss/preset-wind4';
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
  theme: {
    colors: {
      accent: 'var(--color-accent)',
      edge: 'var(--color-border)',
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
