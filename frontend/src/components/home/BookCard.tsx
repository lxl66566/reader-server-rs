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
      class="block w-full cursor-pointer rounded-lg border border-edge bg-surface p-3 text-left transition-colors hover:bg-base"
      onClick={() => {
        props.onClick();
      }}
      type="button"
    >
      <span class="flex items-start justify-between gap-2">
        <span class="truncate text-sm font-medium text-ink">{props.title}</span>
        <span class="shrink-0 rounded border border-edge px-1 text-xs text-muted">
          {t(FORMAT_KEYS[props.format])}
        </span>
      </span>
      <span class="mt-1 block text-xs text-muted">
        {props.author ?? t('unknownAuthor')}
      </span>
      <Show when={props.owner}>
        <span class="mt-1 block text-xs text-muted">
          {t('ownerBy', { username: props.owner! })}
        </span>
      </Show>
      <Show when={props.isPublic}>
        <span class="mt-1 inline-block rounded border border-edge px-1 text-xs text-accent">
          {t('publicTag')}
        </span>
      </Show>
      <Show when={props.percent !== undefined}>
        <span class="mt-2 block h-1.5 w-full overflow-hidden rounded-full bg-edge">
          <span
            class="block h-full rounded-full bg-accent"
            style={{ width: `${props.percent! * 100}%` }}
          />
        </span>
        <span class="mt-1 flex items-center justify-between text-xs text-muted">
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
