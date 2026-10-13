// Theme store: persisted in localStorage (rs_theme), applies data-theme on
// <html>; 'system' follows prefers-color-scheme. Reader pages may override
// colors via inline styles.

import { createEffect, createSignal } from 'solid-js';

import type { ThemeMode } from './api/types';

const media = window.matchMedia('(prefers-color-scheme: dark)');
const STORAGE_KEY = 'rs_theme';

export const THEME_OPTIONS: ThemeMode[] = ['light', 'sepia', 'dark', 'system'];

const THEME_MODES = new Set<string>(THEME_OPTIONS);

function loadStoredTheme(): ThemeMode {
  const stored = localStorage.getItem(STORAGE_KEY);
  return stored !== null && THEME_MODES.has(stored)
    ? (stored as ThemeMode)
    : 'system';
}

const [themeMode, setThemeModeSignal] =
  createSignal<ThemeMode>(loadStoredTheme());

export { themeMode };

export function initTheme(): void {
  media.addEventListener('change', apply);
  createEffect(apply);
}

export function resolvedTheme(): Exclude<ThemeMode, 'system'> {
  const mode = themeMode();
  if (mode !== 'system') return mode;
  return media.matches ? 'dark' : 'light';
}

export function setThemeMode(mode: ThemeMode): void {
  setThemeModeSignal(mode);
  localStorage.setItem(STORAGE_KEY, mode);
}

function apply(): void {
  document.documentElement.dataset.theme = resolvedTheme();
}
