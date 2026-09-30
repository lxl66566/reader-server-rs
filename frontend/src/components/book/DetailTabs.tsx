// Tabbed panels for book chapters, bookmarks and annotations.
import { useNavigate } from '@solidjs/router';
import {
  VsBookmark,
  VsChevronRight,
  VsComment,
  VsListUnordered,
  VsTrash,
} from 'solid-icons/vs';
import { createSignal, For, Show, type Component, type JSX } from 'solid-js';

import type { Annotation, Bookmark, Chapter } from '../../api/types';
import { useI18n } from '../../i18n';
import type { DictKey } from '../../i18n/zh-CN';
import { formatDateTime, formatPercent } from '../../utils/format';
import { EmptyState } from '../ui';
import { ConfirmModal } from './ConfirmModal';

type TabKey = 'annotations' | 'bookmarks' | 'chapters';

const TAB_ORDER: { key: TabKey; label: DictKey }[] = [
  { key: 'chapters', label: 'detailChapters' },
  { key: 'bookmarks', label: 'detailBookmarks' },
  { key: 'annotations', label: 'detailAnnotations' },
];

const TAB_ICONS: Record<TabKey, JSX.Element> = {
  annotations: <VsComment class="size-4" />,
  bookmarks: <VsBookmark class="size-4" />,
  chapters: <VsListUnordered class="size-4" />,
};

export const DetailTabs: Component<{
  annotations: Annotation[];
  bookId: number;
  bookmarks: Bookmark[];
  chapters: Chapter[];
  onDeleteAnnotation: (id: number) => Promise<void>;
  onDeleteBookmark: (id: number) => Promise<void>;
}> = props => {
  const { locale, t } = useI18n();
  const navigate = useNavigate();
  const [tab, setTab] = createSignal<TabKey>('chapters');
  const [pendingBookmark, setPendingBookmark] = createSignal<null | number>(
    null,
  );
  const [pendingAnnotation, setPendingAnnotation] = createSignal<null | number>(
    null,
  );

  const openChapter = (chapter: Chapter) => {
    navigate(`/read/${props.bookId}?chapter=${chapter.chapter_id}`);
  };
  const openBookmark = (bookmark: Bookmark) => {
    navigate(
      `/read/${props.bookId}?locator=${encodeURIComponent(bookmark.locator)}`,
    );
  };
  const openAnnotation = (annotation: Annotation) => {
    navigate(
      `/read/${props.bookId}?locator=${encodeURIComponent(annotation.start_locator)}`,
    );
  };

  return (
    <section class="border-edge bg-surface overflow-hidden rounded-lg border">
      <div class="border-edge flex border-b" role="tablist">
        <For each={TAB_ORDER}>
          {item => (
            <button
              aria-selected={tab() === item.key}
              class={`flex min-h-11 flex-1 cursor-pointer items-center justify-center gap-1.5 border-b-2 text-sm transition-colors ${
                tab() === item.key
                  ? 'border-accent text-ink'
                  : 'text-muted hover:text-ink border-transparent'
              }`}
              onClick={() => setTab(item.key)}
              role="tab"
              type="button"
            >
              {TAB_ICONS[item.key]}
              {t(item.label)}
            </button>
          )}
        </For>
      </div>

      <div class="p-2 sm:p-3">
        <Show when={tab() === 'chapters'}>
          <ChaptersPanel chapters={props.chapters} onOpen={openChapter} />
        </Show>
        <Show when={tab() === 'bookmarks'}>
          <BookmarksPanel
            bookmarks={props.bookmarks}
            locale={locale()}
            onDelete={setPendingBookmark}
            onOpen={openBookmark}
          />
        </Show>
        <Show when={tab() === 'annotations'}>
          <AnnotationsPanel
            annotations={props.annotations}
            locale={locale()}
            onDelete={setPendingAnnotation}
            onOpen={openAnnotation}
          />
        </Show>
      </div>

      <ConfirmModal
        message={t('readerDeleteConfirm')}
        onClose={() => setPendingBookmark(null)}
        onConfirm={() => {
          const id = pendingBookmark();
          return id === null ? undefined : props.onDeleteBookmark(id);
        }}
        open={pendingBookmark() !== null}
      />
      <ConfirmModal
        message={t('readerDeleteConfirm')}
        onClose={() => setPendingAnnotation(null)}
        onConfirm={() => {
          const id = pendingAnnotation();
          return id === null ? undefined : props.onDeleteAnnotation(id);
        }}
        open={pendingAnnotation() !== null}
      />
    </section>
  );
};

function AnnotationsPanel(props: {
  annotations: Annotation[];
  locale: string;
  onDelete: (id: number) => void;
  onOpen: (annotation: Annotation) => void;
}) {
  const { t } = useI18n();
  return (
    <Show
      fallback={<EmptyState text={t('noAnnotations')} />}
      when={props.annotations.length > 0}
    >
      <ul class="divide-edge divide-y">
        <For each={props.annotations}>
          {annotation => (
            <li class="py-3">
              <div class="flex items-start gap-2">
                <button
                  class="min-w-0 flex-1 text-left"
                  onClick={() => {
                    props.onOpen(annotation);
                  }}
                  type="button"
                >
                  <span class="flex flex-wrap items-center gap-2">
                    <span class="border-edge bg-page rounded-full border px-2 py-0.5 text-xs">
                      {annotation.kind === 'note'
                        ? t('kindNote')
                        : t('kindHighlight')}
                    </span>
                    <span class="text-muted text-xs">
                      {t('progressPercent', {
                        percent: formatPercent(annotation.start_percent),
                      })}
                    </span>
                  </span>
                  <Show when={annotation.excerpt}>
                    <p class="border-edge text-muted mt-1.5 line-clamp-2 border-l-2 pl-2 text-sm italic">
                      {annotation.excerpt}
                    </p>
                  </Show>
                  <Show when={annotation.content}>
                    <p class="text-ink mt-1 line-clamp-3 text-sm">
                      {annotation.content}
                    </p>
                  </Show>
                </button>
                {/* bookmarks/annotations are per-user: everyone deletes their own */}
                <button
                  aria-label={t('delete')}
                  class="text-muted flex size-10 shrink-0 cursor-pointer items-center justify-center transition-colors hover:text-red-700 dark:hover:text-red-400"
                  onClick={() => {
                    props.onDelete(annotation.id);
                  }}
                  type="button"
                >
                  <VsTrash class="size-4" />
                </button>
              </div>
            </li>
          )}
        </For>
      </ul>
    </Show>
  );
}

function BookmarksPanel(props: {
  bookmarks: Bookmark[];
  locale: string;
  onDelete: (id: number) => void;
  onOpen: (bookmark: Bookmark) => void;
}) {
  const { t } = useI18n();
  return (
    <Show
      fallback={<EmptyState text={t('noBookmarks')} />}
      when={props.bookmarks.length > 0}
    >
      <ul class="divide-edge divide-y">
        <For each={props.bookmarks}>
          {bookmark => (
            <li class="flex min-h-11 items-center gap-2">
              <button
                class="min-w-0 flex-1 py-2 text-left"
                onClick={() => {
                  props.onOpen(bookmark);
                }}
                type="button"
              >
                <span class="block truncate text-sm">
                  {bookmark.label ??
                    t('progressPercent', {
                      percent: formatPercent(bookmark.percent),
                    })}
                </span>
                <span class="text-muted block text-xs">
                  {formatDateTime(bookmark.created_at, props.locale)}
                </span>
              </button>
              <button
                aria-label={t('delete')}
                class="text-muted flex size-10 shrink-0 cursor-pointer items-center justify-center transition-colors hover:text-red-700 dark:hover:text-red-400"
                onClick={() => {
                  props.onDelete(bookmark.id);
                }}
                type="button"
              >
                <VsTrash class="size-4" />
              </button>
            </li>
          )}
        </For>
      </ul>
    </Show>
  );
}

function ChaptersPanel(props: {
  chapters: Chapter[];
  onOpen: (chapter: Chapter) => void;
}) {
  const { t } = useI18n();
  return (
    <Show
      fallback={<EmptyState text={t('noChapters')} />}
      when={props.chapters.length > 0}
    >
      <ol class="divide-edge divide-y">
        <For each={props.chapters}>
          {chapter => (
            <li>
              <button
                class="hover:bg-page flex min-h-11 w-full cursor-pointer items-center gap-2 py-1 text-left text-sm"
                onClick={() => {
                  props.onOpen(chapter);
                }}
                type="button"
              >
                <VsChevronRight class="text-muted size-4 shrink-0" />
                <span class="truncate">{chapter.title}</span>
              </button>
            </li>
          )}
        </For>
      </ol>
    </Show>
  );
}
