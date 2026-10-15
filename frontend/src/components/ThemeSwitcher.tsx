// 导航栏主题切换：单按钮显示当前模式，悬浮/点击展开选项菜单。
// 本地立即生效并持久化；登录态的服务端同步由 settings store 处理。

import type { IconTypes } from 'solid-icons';
// 跟随系统用半日半月图标（Docusaurus/next-themes 等的通用"auto"标准图标）
import {
  TbOutlineBook,
  TbOutlineMoon,
  TbOutlineSun,
  TbOutlineSunMoon,
} from 'solid-icons/tb';
import {
  createEffect,
  createSignal,
  For,
  onCleanup,
  Show,
  type Component,
} from 'solid-js';
import { Dynamic } from 'solid-js/web';

import type { ThemeMode } from '../api/types';
import { useI18n } from '../i18n';
import type { DictKey } from '../i18n/zh-CN';
import { useSettings } from '../stores/settings';
import { themeMode, THEME_OPTIONS } from '../theme';

const THEME_ICONS: Record<ThemeMode, IconTypes> = {
  dark: TbOutlineMoon,
  light: TbOutlineSun,
  sepia: TbOutlineBook,
  system: TbOutlineSunMoon,
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
  const [open, setOpen] = createSignal(false);
  let wrapRef: HTMLDivElement | undefined;

  // 悬浮打开；关闭加短延时，跨过按钮与菜单间的缝隙时菜单不闪关
  let closeTimer: ReturnType<typeof setTimeout> | undefined;
  const cancelClose = () => {
    clearTimeout(closeTimer);
    closeTimer = undefined;
  };
  const scheduleClose = () => {
    cancelClose();
    closeTimer = setTimeout(() => setOpen(false), 120);
  };
  onCleanup(cancelClose);

  // 悬浮之外还有触摸与键盘路径：打开期间点击外部或按 Escape 关闭
  createEffect(() => {
    if (!open()) return;
    const onPointerDown = (e: PointerEvent) => {
      if (!wrapRef?.contains(e.target as Node)) setOpen(false);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    onCleanup(() => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    });
  });

  return (
    <div
      class="relative"
      onFocusOut={e => {
        if (!wrapRef?.contains(e.relatedTarget as Node)) setOpen(false);
      }}
      onMouseEnter={() => {
        cancelClose();
        setOpen(true);
      }}
      onMouseLeave={scheduleClose}
      ref={wrapRef}
    >
      <button
        aria-expanded={open()}
        aria-haspopup="menu"
        aria-label={t('settingsTheme')}
        class="text-muted hover:bg-page hover:text-ink flex cursor-pointer rounded p-1.5 transition-colors"
        onClick={() => {
          // 只开不关：悬浮已打开时点击若 toggle 会立即关掉菜单；
          // 触摸端经选项/点击外部/Escape 关闭
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        title={t(THEME_LABELS[themeMode()])}
        type="button"
      >
        <Dynamic class="size-4" component={THEME_ICONS[themeMode()]} />
      </button>
      <Show when={open()}>
        <div
          aria-label={t('settingsTheme')}
          class="border-edge bg-surface rs-fade-in absolute top-full right-0 z-50 mt-1 flex min-w-28 flex-col rounded-lg border p-1 shadow-lg"
          onMouseEnter={cancelClose}
          role="menu"
        >
          <For each={THEME_OPTIONS}>
            {mode => {
              const Icon = THEME_ICONS[mode];
              const active = () => themeMode() === mode;
              return (
                <button
                  aria-checked={active()}
                  aria-label={t(THEME_LABELS[mode])}
                  class={`flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm whitespace-nowrap transition-colors ${
                    active()
                      ? 'bg-page text-ink'
                      : 'text-muted hover:bg-page hover:text-ink'
                  }`}
                  onClick={() => {
                    setTheme(mode);
                    setOpen(false);
                  }}
                  role="menuitemradio"
                  type="button"
                >
                  <Icon class="size-4 shrink-0" />
                  <span>{t(THEME_LABELS[mode])}</span>
                </button>
              );
            }}
          </For>
        </div>
      </Show>
    </div>
  );
};
