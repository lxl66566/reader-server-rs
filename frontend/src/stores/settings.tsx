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
import { setThemeMode } from '../theme';

const FALLBACK: ReadingSettings = {
  background_color: '#faf9f6',
  font_family: 'default',
  font_size: 18,
  letter_spacing: 0,
  line_height: 1.8,
  pagination_mode: 'paginate',
  paragraph_spacing: 0.8,
  reading_width: 40,
  simplified_chinese: false,
  text_color: '#2b2a26',
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
  const [settings, setSettings] = createStore<ReadingSettings>({ ...FALLBACK });
  const [remote, { refetch }] = createResource(async () => {
    try {
      return await readingApi.settings();
    } catch {
      // logged-out visitors get local defaults
      return undefined;
    }
  });

  const applyRemote = (value: ReadingSettings) => {
    setSettings(value);
    setThemeMode(value.theme);
  };

  // apply once loaded (createEffect: remote() resolves asynchronously)
  createEffect(() => {
    const value = remote();
    if (value) applyRemote(value);
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
