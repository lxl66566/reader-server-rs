// txt 章节切分正则输入：空 = 内置默认规则，用 new RegExp 做轻量即时校验（服务端 fancy-regex 为准）
import { createMemo, Show, type ParentComponent } from 'solid-js';

import { useI18n } from '../i18n';
import { Input } from './ui';

/** 轻量校验：空串视为有效（使用默认规则） */
export function isValidChapterPattern(pattern: string): boolean {
  const trimmed = pattern.trim();
  if (!trimmed) return true;
  try {
    // 能构造即可；test 消费实例本身，避免闲置变量
    new RegExp(trimmed).test('');
    return true;
  } catch {
    return false;
  }
}

export const ChapterPatternField: ParentComponent<{
  onInput: (value: string) => void;
  value: string;
}> = props => {
  const { t } = useI18n();
  const invalid = createMemo(() => !isValidChapterPattern(props.value));

  return (
    <label class="block text-sm">
      <span class="text-muted mb-1 block">{t('chapterPatternLabel')}</span>
      <Input
        onInput={props.onInput}
        placeholder={t('chapterPatternPlaceholder')}
        value={props.value}
      />
      <Show when={invalid()}>
        <span class="mt-1 block text-xs text-red-600 dark:text-red-400">
          {t('chapterPatternInvalid')}
        </span>
      </Show>
    </label>
  );
};
