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

const initialMode = loadStoredTheme();
const [themeMode, setThemeModeSignal] = createSignal<ThemeMode>(initialMode);

export { themeMode };

export function initTheme(): void {
  media.addEventListener('change', apply);
  createEffect(apply);
}

function computeResolved(mode: ThemeMode): Exclude<ThemeMode, 'system'> {
  if (mode !== 'system') return mode;
  return media.matches ? 'dark' : 'light';
}

// 响应式：themeMode 变化与 system 模式下媒体查询翻转都会更新
const [resolved, setResolved] = createSignal<Exclude<ThemeMode, 'system'>>(
  computeResolved(initialMode),
);

export const resolvedTheme = resolved;

export function setThemeMode(mode: ThemeMode): void {
  setThemeModeSignal(mode);
  localStorage.setItem(STORAGE_KEY, mode);
}

function apply(): void {
  const r = computeResolved(themeMode());
  document.documentElement.dataset.theme = r;
  setResolved(r);
}
