// Applies data-theme on <html> based on ReadingSettings.theme ('system' follows
// prefers-color-scheme). Reader pages may override colors via inline styles.

import { createEffect, createSignal } from 'solid-js';

import type { ThemeMode } from './api/types';

const media = window.matchMedia('(prefers-color-scheme: dark)');

const [themeMode, setThemeModeSignal] = createSignal<ThemeMode>('system');

export { themeMode };

export function initTheme(): void {
  media.addEventListener('change', apply);
  createEffect(apply);
}

export function setThemeMode(mode: ThemeMode): void {
  setThemeModeSignal(mode);
}

function apply(): void {
  document.documentElement.dataset.theme = resolved();
}

function resolved(): Exclude<ThemeMode, 'system'> {
  const mode = themeMode();
  if (mode !== 'system') return mode;
  return media.matches ? 'dark' : 'light';
}
