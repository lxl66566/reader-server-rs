// Book card used across library / recommend / public-library grids.
// Clicking navigates to the book detail page (handled by the parent).

import { Show, type ParentComponent } from 'solid-js';

import type { BookFormat } from '../../api/types';
import { useI18n } from '../../i18n';
import type { DictKey } from '../../i18n/zh-CN';
import { formatDuration, formatPercent } from '../../utils/format';

const FORMAT_KEYS: Record<BookFormat, DictKey> = {
  epub: 'formatEpub',
  pdf: 'formatPdf',
  txt: 'formatTxt',
};

export const BookCard: ParentComponent<{
  author: null | string;
  format: BookFormat;
  isPublic?: boolean;
  onClick: () => void;
  owner?: string;
  percent?: number;
  readingTime?: number;
  title: string;
}> = props => {
  const { t } = useI18n();
  return (
    <button
      class="border-edge bg-surface hover:bg-page block w-full cursor-pointer rounded-lg border p-3 text-left transition-colors"
      onClick={() => {
        props.onClick();
      }}
      type="button"
    >
      <span class="flex items-start justify-between gap-2">
        <span class="text-ink truncate text-sm font-medium">{props.title}</span>
        <span class="border-edge text-muted shrink-0 rounded border px-1 text-xs">
          {t(FORMAT_KEYS[props.format])}
        </span>
      </span>
      <span class="text-muted mt-1 block text-xs">
        {props.author ?? t('unknownAuthor')}
      </span>
      <Show when={props.owner}>
        <span class="text-muted mt-1 block text-xs">
          {t('ownerBy', { username: props.owner! })}
        </span>
      </Show>
      <Show when={props.isPublic}>
        <span class="border-edge text-accent mt-1 inline-block rounded border px-1 text-xs">
          {t('publicTag')}
        </span>
      </Show>
      <Show when={props.percent !== undefined}>
        <span class="bg-edge mt-2 block h-1.5 w-full overflow-hidden rounded-full">
          <span
            class="bg-accent block h-full rounded-full"
            style={{ width: `${props.percent! * 100}%` }}
          />
        </span>
        <span class="text-muted mt-1 flex items-center justify-between text-xs">
          <span>
            {t('progressPercent', { percent: formatPercent(props.percent!) })}
          </span>
          <Show when={props.readingTime !== undefined}>
            <span>{formatDuration(props.readingTime!, t)}</span>
          </Show>
        </span>
      </Show>
    </button>
  );
};
