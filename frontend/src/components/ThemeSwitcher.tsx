// 导航栏主题切换：图标分段按钮，游客与登录用户均可用。
// 本地立即生效并持久化；登录态的服务端同步由 settings store 处理。

import type { IconTypes } from 'solid-icons';
import { BsBookHalf, BsDisplay, BsMoon, BsSun } from 'solid-icons/bs';
import { For, type Component } from 'solid-js';

import type { ThemeMode } from '../api/types';
import { useI18n } from '../i18n';
import type { DictKey } from '../i18n/zh-CN';
import { useSettings } from '../stores/settings';
import { themeMode, THEME_OPTIONS } from '../theme';

const THEME_ICONS: Record<ThemeMode, IconTypes> = {
  dark: BsMoon,
  light: BsSun,
  sepia: BsBookHalf,
  system: BsDisplay,
};

const THEME_LABELS: Record<ThemeMode, DictKey> = {
  dark: 'themeDark',
  light: 'themeLight',
  sepia: 'themeSepia',
  system: 'themeSystem',
};

export const ThemeSwitcher: Component = () => {
  const { t } = useI18n();
  const { setTheme } = useSettings();

  return (
    <div
      aria-label={t('settingsTheme')}
      class="border-edge flex items-center gap-0.5 rounded border p-0.5"
      role="group"
    >
      <For each={THEME_OPTIONS}>
        {mode => {
          const Icon = THEME_ICONS[mode];
          const active = () => themeMode() === mode;
          return (
            <button
              aria-label={t(THEME_LABELS[mode])}
              aria-pressed={active()}
              class={`flex cursor-pointer rounded p-1 ${
                active()
                  ? 'bg-page text-ink'
                  : 'text-muted hover:bg-page hover:text-ink'
              }`}
              onClick={() => {
                setTheme(mode);
              }}
              title={t(THEME_LABELS[mode])}
              type="button"
            >
              <Icon class="size-4" />
            </button>
          );
        }}
      </For>
    </div>
  );
};
