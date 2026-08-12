import { presetWind4, type Theme } from '@unocss/preset-wind4';
import { defineConfig, type UserConfig } from 'unocss';

// Theme-aware colors map to CSS variables defined in index.css,
// so light/sepia/dark switching needs no class regeneration.
const config: UserConfig<Theme> = defineConfig({
  presets: [presetWind4()],
  theme: {
    colors: {
      accent: 'var(--color-accent)',
      base: 'var(--color-bg)',
      edge: 'var(--color-border)',
      ink: 'var(--color-fg)',
      muted: 'var(--color-muted)',
      paper: 'var(--color-paper)',
      surface: 'var(--color-surface)',
    },
  },
});

export default config;
