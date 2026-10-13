// Dictionary-based i18n: flat keys, {placeholder} interpolation, zh-CN default.

import {
  createContext,
  createSignal,
  useContext,
  type ParentComponent,
} from 'solid-js';

import en from './en';
import zhCN, { type DictKey } from './zh-CN';

export const LOCALES = ['zh-CN', 'en'] as const;
export type Locale = (typeof LOCALES)[number];

const DICTS: Record<Locale, Record<DictKey, string>> = {
  en,
  'zh-CN': zhCN,
};

const STORAGE_KEY = 'rs_locale';

interface I18nContextValue {
  locale: () => Locale;
  setLocale: (locale: Locale) => void;
  t: (key: DictKey, params?: Record<string, number | string>) => string;
}

/**
 * 组件树外（如 api 层）的翻译入口：按 localStorage 中的当前语言查表，
 * 与 I18nProvider 使用同一存储键，语言切换后下一次调用即生效。
 */
export function translateGlobal(
  key: DictKey,
  params?: Record<string, number | string>,
): string {
  let text = DICTS[detectLocale()][key];
  if (params) {
    for (const [name, value] of Object.entries(params)) {
      text = text.replaceAll(`{${name}}`, String(value));
    }
  }
  return text;
}

function detectLocale(): Locale {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === 'zh-CN' || stored === 'en') return stored;
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en';
}

const I18nContext = createContext<I18nContextValue>();

export const I18nProvider: ParentComponent = props => {
  const [locale, setLocaleSignal] = createSignal<Locale>(detectLocale());

  const setLocale = (next: Locale) => {
    localStorage.setItem(STORAGE_KEY, next);
    document.documentElement.lang = next;
    setLocaleSignal(next);
  };

  const t: I18nContextValue['t'] = (key, params) => {
    let text = DICTS[locale()][key];
    if (params) {
      for (const [name, value] of Object.entries(params)) {
        text = text.replaceAll(`{${name}}`, String(value));
      }
    }
    return text;
  };

  return (
    <I18nContext.Provider value={{ locale, setLocale, t }}>
      {props.children}
    </I18nContext.Provider>
  );
};

export function useI18n(): I18nContextValue {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error('useI18n must be used within I18nProvider');
  return ctx;
}
