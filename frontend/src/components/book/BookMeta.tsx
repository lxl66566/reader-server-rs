// Book header: identity, format / visibility marks, stats row and read entry.
import { VsBook } from 'solid-icons/vs';
import { Show, type Component } from 'solid-js';
import type { BookDetail, BookFormat } from '../../api/types';
import { useI18n } from '../../i18n';
import type { DictKey } from '../../i18n/zh-CN';
import {
  formatDateTime,
  formatDuration,
  formatPercent,
} from '../../utils/format';
import { Button } from '../ui';

const FORMAT_LABELS: Record<BookFormat, DictKey> = {
  epub: 'formatEpub',
  pdf: 'formatPdf',
  txt: 'formatTxt',
};

export const BookMeta: Component<{
  book: BookDetail;
  onRead: () => void;
}> = props => {
  const { locale, t } = useI18n();

  return (
    <header class="space-y-4">
      <div class="flex flex-wrap items-start gap-3">
        <div class="min-w-0 flex-1">
          <h1 class="text-xl font-semibold leading-snug">{props.book.title}</h1>
          <p class="mt-1 text-sm text-muted">
            {props.book.author
              ? t('bookBy', { author: props.book.author })
              : t('unknownAuthor')}
          </p>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <span class="rounded-full border border-edge bg-base px-2 py-0.5 text-xs">
            {t(FORMAT_LABELS[props.book.format])}
          </span>
          <Show when={props.book.is_public}>
            <span class="rounded-full border border-edge bg-base px-2 py-0.5 text-xs">
              {t('publicTag')}
            </span>
          </Show>
        </div>
      </div>

      <dl class="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted">
        <div class="flex items-center gap-1">
          <dt class="sr-only">{t('createdAt')}</dt>
          <dd>{formatDateTime(props.book.created_at, locale())}</dd>
        </div>
        <div>
          {props.book.format === 'pdf'
            ? t('totalPages', { count: props.book.total_pages })
            : t('totalChars', { count: props.book.total_chars })}
        </div>
        <div>{formatDuration(props.book.reading_time, t)}</div>
        <div>
          {t('progressPercent', { percent: formatPercent(props.book.percent) })}
        </div>
      </dl>

      <Button class="min-h-10 px-5" onClick={props.onRead}>
        <VsBook class="size-4" />
        {props.book.percent > 0 ? t('continueReading') : t('startReading')}
      </Button>
    </header>
  );
};
