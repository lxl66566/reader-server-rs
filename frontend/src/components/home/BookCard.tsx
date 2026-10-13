// Book card used across library / recommend / public-library grids.
// Clicking the card opens the reader directly; the corner icon opens the
// book detail page. The whole-card hit area is an overlay button so the
// corner icon can stay a real sibling button (no nested buttons).

import { VsInfo } from 'solid-icons/vs';
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
  onDetail: () => void;
  onRead: () => void;
  owner?: string;
  percent?: number;
  readingTime?: number;
  title: string;
}> = props => {
  const { t } = useI18n();
  return (
    <div class="border-edge bg-surface hover:bg-page relative w-full rounded-lg border p-3 transition-colors">
      <button
        aria-label={t('readBook', { title: props.title })}
        class="absolute inset-0 cursor-pointer rounded-lg"
        onClick={() => {
          props.onRead();
        }}
        title={t('readBook', { title: props.title })}
        type="button"
      />
      <div class="mt-1 flex items-center gap-1">
        <span class="text-ink min-w-0 flex-1 truncate text-sm font-medium">
          {props.title}
        </span>
        <button
          aria-label={t('viewDetail')}
          class="text-muted hover:bg-edge hover:text-ink relative z-10 -m-1.5 shrink-0 cursor-pointer rounded p-1.5 transition-colors"
          onClick={() => {
            props.onDetail();
          }}
          title={t('viewDetail')}
          type="button"
        >
          <VsInfo class="size-4" />
        </button>
      </div>
      <span class="text-muted mt-1 flex min-w-0 items-center justify-between gap-2 text-xs">
        <span class="min-w-0 flex-1 truncate">
          {props.author ?? t('unknownAuthor')}
        </span>
        <Show when={props.owner}>
          <span class="min-w-0 shrink truncate">
            {t('ownerBy', { username: props.owner! })}
          </span>
        </Show>
        <span class="flex shrink-0 items-center gap-1">
          <Show when={props.isPublic}>
            <span class="border-edge text-accent rounded border px-1">
              {t('publicTag')}
            </span>
          </Show>
          <span class="border-edge text-muted rounded border px-1">
            {t(FORMAT_KEYS[props.format])}
          </span>
        </span>
      </span>
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
    </div>
  );
};
