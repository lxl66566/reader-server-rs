// Reading settings store: server-synced, locally cached, drives theme + readers.

import {
  createContext,
  createEffect,
  createResource,
  useContext,
  type ParentComponent,
} from 'solid-js';
import { createStore } from 'solid-js/store';

import { readingApi } from '../api';
import type { PaginationMode, ReadingSettings, ThemeMode } from '../api/types';
import { resolvedTheme, setThemeMode } from '../theme';
import { useAuth } from './auth';

// 服务端 schema 默认色，作为"用户未自定义"的默认值
const SERVER_DEFAULT_COLORS = {
  background: '#F5F5DC',
  text: '#000000',
} as const;
// 初始化默认色：亮/暗主题各取色板中最白与最黑的两个
const THEME_DEFAULT_COLORS = {
  dark: { background_color: '#201f1b', text_color: '#e8e6e0' },
  light: { background_color: '#eef1f5', text_color: '#2b2a26' },
} as const;

const themeDefaultColors = () =>
  THEME_DEFAULT_COLORS[resolvedTheme() === 'dark' ? 'dark' : 'light'];

const FALLBACK: ReadingSettings = {
  ...themeDefaultColors(),
  // 与服务端 schema 默认值（DEFAULT 'none'）保持一致
  chinese_conversion: 'none',
  font_family: 'default',
  font_size: 18,
  letter_spacing: 0,
  line_height: 1.8,
  pagination_mode: 'paginate',
  paragraph_spacing: 0.8,
  reading_width: 40,
  text_indent: 2,
  theme: 'system',
  timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
};

interface SettingsContextValue {
  ready: () => boolean;
  reload: () => void;
  settings: ReadingSettings;
  /** optimistic local update + debounced server sync */
  update: (patch: Partial<ReadingSettings>) => void;
}

const SettingsContext = createContext<SettingsContextValue>();

export const SettingsProvider: ParentComponent = props => {
  const auth = useAuth();
  const [settings, setSettings] = createStore<ReadingSettings>({ ...FALLBACK });
  // 以登录态为 source：登录前请求必然 401，登录成功后靠 source 变化重新拉取
  const [remote, { refetch }] = createResource(
    () => auth.isLoggedIn(),
    async (loggedIn: boolean) => {
      // refetch 会绕过 source 门控强制执行 fetcher：登出后必须短路，
      // 否则会多发一次不带 token、注定 401 的请求
      if (!loggedIn) return undefined;
      try {
        return await readingApi.settings();
      } catch {
        return undefined;
      }
    },
  );

  const applyRemote = (value: ReadingSettings) => {
    setSettings(value);
    setThemeMode(value.theme);
  };

  // apply once loaded (createEffect: remote() resolves asynchronously)
  createEffect(() => {
    // 加载中保持现状，避免已登录用户被访客默认值闪一下
    if (remote.loading) return;
    const value = remote();
    // 未登录或拉取失败：回到访客默认，避免残留上个用户的设置
    if (!value || !auth.isLoggedIn()) {
      applyRemote({ ...FALLBACK, ...themeDefaultColors() });
      return;
    }
    // 初始化：颜色仍是服务端默认（用户未自定义过）时，按当前主题写入默认色
    if (
      value.background_color === SERVER_DEFAULT_COLORS.background &&
      value.text_color === SERVER_DEFAULT_COLORS.text
    ) {
      update(themeDefaultColors());
      return;
    }
    applyRemote(value);
  });

  let syncTimer: ReturnType<typeof setTimeout> | undefined;
  let pending: Partial<ReadingSettings> = {};

  const update: SettingsContextValue['update'] = patch => {
    setSettings(patch);
    if (patch.theme !== undefined) setThemeMode(patch.theme);
    pending = { ...pending, ...patch };
    clearTimeout(syncTimer);
    syncTimer = setTimeout(() => {
      const body = pending;
      pending = {};
      void readingApi.updateSettings(body).catch(() => {
        // keep optimistic state; next change will retry the merge
      });
    }, 500);
  };

  const value: SettingsContextValue = {
    ready: () => remote.state === 'ready' || remote.state === 'errored',
    reload: () => {
      // the createEffect above applies the fresh value once resolved
      void refetch();
    },
    settings,
    update,
  };

  return (
    <SettingsContext.Provider value={value}>
      {props.children}
    </SettingsContext.Provider>
  );
};

export function useSettings(): SettingsContextValue {
  const ctx = useContext(SettingsContext);
  if (!ctx) throw new Error('useSettings must be used within SettingsProvider');
  return ctx;
}

// Shared option lists for settings panels (reader + settings page).
export const FONT_FAMILY_OPTIONS = ['default', 'serif', 'sans'] as const;
export type FontFamilyOption = (typeof FONT_FAMILY_OPTIONS)[number];

export const THEME_OPTIONS: ThemeMode[] = ['light', 'sepia', 'dark', 'system'];
export const PAGINATION_OPTIONS: PaginationMode[] = ['scroll', 'paginate'];

export function fontFamilyCss(option: string): string {
  switch (option) {
    case 'sans':
      return '-apple-system, "PingFang SC", "Microsoft YaHei", sans-serif';
    case 'serif':
      return 'Georgia, "Songti SC", "SimSun", serif';
    default:
      return '';
  }
}
