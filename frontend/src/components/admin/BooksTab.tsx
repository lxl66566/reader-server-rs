// Global book management: search, paginated list, edit and delete for admins.

import { VsSearch } from 'solid-icons/vs';
import {
  createEffect,
  createResource,
  createSignal,
  For,
  Show,
  type Component,
} from 'solid-js';
import toast from 'solid-toast';

import { adminApi } from '../../api';
import type { AdminBookItem, BookFormat } from '../../api/types';
import { useI18n } from '../../i18n';
import type { DictKey } from '../../i18n/zh-CN';
import { formatDateTime, formatFileSize } from '../../utils/format';
import { toastError } from '../../utils/toast';
import {
  Button,
  EmptyState,
  Input,
  Modal,
  SectionCard,
  Spinner,
  Toggle,
} from '../ui';
import { ConfirmDialog } from './ConfirmDialog';
import { isUnauthorized } from './utils';

const PAGE_LIMIT = 20;

const FORMAT_LABELS: Record<BookFormat, DictKey> = {
  epub: 'formatEpub',
  pdf: 'formatPdf',
  txt: 'formatTxt',
};

export const BooksTab: Component<{ onUnauthorized: () => void }> = props => {
  const { locale, t } = useI18n();
  const [page, setPage] = createSignal(1);
  const [searchInput, setSearchInput] = createSignal('');
  // 回车提交后才触发请求，避免每敲一个字符就打一次接口
  const [query, setQuery] = createSignal('');

  const [books, { refetch }] = createResource(
    () => ({ page: page(), q: query() }),
    async source => {
      try {
        return await adminApi.books(source.page, PAGE_LIMIT, source.q);
      } catch (err) {
        if (isUnauthorized(err)) props.onUnauthorized();
        throw err;
      }
    },
  );
  const list = () => books()?.books ?? [];

  // an expired admin token surfaces as 401 on any request
  createEffect(() => {
    if (isUnauthorized(books.error)) props.onUnauthorized();
  });

  const total = () => books()?.total ?? 0;
  const totalPages = () => Math.max(1, Math.ceil(total() / PAGE_LIMIT));
  const hasPrev = () => page() > 1;
  const hasNext = () => total() > page() * PAGE_LIMIT;

  const submitSearch = () => {
    setPage(1);
    setQuery(searchInput().trim());
  };

  // ============ 编辑 ============

  const [editing, setEditing] = createSignal<AdminBookItem | null>(null);
  const [editTitle, setEditTitle] = createSignal('');
  const [editAuthor, setEditAuthor] = createSignal('');
  const [editPublic, setEditPublic] = createSignal(false);
  const [saving, setSaving] = createSignal(false);

  const openEdit = (book: AdminBookItem) => {
    setEditing(book);
    setEditTitle(book.title);
    setEditAuthor(book.author ?? '');
    setEditPublic(book.is_public);
  };

  const closeEdit = () => {
    setEditing(null);
  };

  const saveEdit = async () => {
    const book = editing();
    if (!book || !editTitle().trim()) return;
    setSaving(true);
    try {
      await adminApi.updateBook(book.book_id, {
        author: editAuthor().trim() || undefined,
        is_public: editPublic(),
        title: editTitle().trim(),
      });
      toast.success(t('adminBookUpdated'));
      closeEdit();
      await refetch();
    } catch (err) {
      if (isUnauthorized(err)) props.onUnauthorized();
      toastError(err, t);
    } finally {
      setSaving(false);
    }
  };

  // ============ 删除 ============

  const [deleting, setDeleting] = createSignal<AdminBookItem | null>(null);

  const confirmDelete = async () => {
    const book = deleting();
    if (!book) return;
    try {
      await adminApi.deleteBook(book.book_id);
      toast.success(t('adminBookDeleted'));
      setDeleting(null);
      await refetch();
    } catch (err) {
      if (isUnauthorized(err)) props.onUnauthorized();
      toastError(err, t);
    }
  };

  return (
    <SectionCard
      actions={
        <div class="flex items-center gap-1">
          <Input
            class="w-40 sm:w-56"
            onInput={setSearchInput}
            onKeyDown={event => {
              if (event.key === 'Enter') submitSearch();
            }}
            placeholder={t('adminBooksSearchPlaceholder')}
            size="sm"
            value={searchInput()}
          />
          <Button class="px-2" onClick={submitSearch} variant="ghost">
            <VsSearch class="size-4" />
          </Button>
        </div>
      }
      title={t('adminTabBooks')}
    >
      <Show fallback={<EmptyState text={t('none')} />} when={list().length > 0}>
        <div class="overflow-x-auto">
          <table class="w-full min-w-[760px] text-sm">
            <thead>
              <tr class="text-muted text-left">
                <th class="pr-3 pb-2 font-normal">{t('titleLabel')}</th>
                <th class="pr-3 pb-2 font-normal">{t('authorLabel')}</th>
                <th class="pr-3 pb-2 font-normal">{t('username')}</th>
                <th class="pr-3 pb-2 font-normal">{t('format')}</th>
                <th class="pr-3 pb-2 font-normal">{t('size')}</th>
                <th class="pr-3 pb-2 font-normal">{t('visibility')}</th>
                <th class="pr-3 pb-2 font-normal">{t('createdAt')}</th>
                <th class="pb-2 font-normal">{t('actions')}</th>
              </tr>
            </thead>
            <tbody>
              <For each={list()}>
                {(item: AdminBookItem) => (
                  <tr class="border-edge border-t align-middle">
                    <td class="max-w-40 truncate py-2 pr-3 font-medium">
                      {item.title}
                    </td>
                    <td class="text-muted py-2 pr-3">
                      {item.author ?? t('unknownAuthor')}
                    </td>
                    <td class="py-2 pr-3">{item.owner_username}</td>
                    <td class="py-2 pr-3">
                      <span class="border-edge bg-page rounded-full border px-2 py-0.5 text-xs">
                        {t(FORMAT_LABELS[item.format])}
                      </span>
                    </td>
                    <td class="text-muted py-2 pr-3">
                      {item.file_size === null
                        ? '—'
                        : formatFileSize(item.file_size)}
                    </td>
                    <td class="py-2 pr-3">
                      <span
                        class={`rounded px-1.5 py-0.5 text-xs ${
                          item.is_public
                            ? 'bg-accent/15 text-accent'
                            : 'border-edge bg-surface text-muted border'
                        }`}
                      >
                        {item.is_public ? t('publicTag') : t('privateTag')}
                      </span>
                    </td>
                    <td class="text-muted py-2 pr-3">
                      {formatDateTime(item.created_at, locale())}
                    </td>
                    <td class="py-2">
                      <div class="flex flex-wrap gap-1">
                        <RowButton
                          label={t('editBook')}
                          onClick={() => {
                            openEdit(item);
                          }}
                        />
                        <RowButton
                          label={t('delete')}
                          onClick={() => setDeleting(item)}
                        />
                      </div>
                    </td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </div>
        <div class="mt-4 flex items-center justify-between">
          <Button
            disabled={!hasPrev()}
            onClick={() => setPage(p => p - 1)}
            variant="ghost"
          >
            {t('prevPage')}
          </Button>
          <span class="text-muted text-sm">
            {t('pageIndicator', { page: page(), total: totalPages() })}
          </span>
          <Button
            disabled={!hasNext()}
            onClick={() => setPage(p => p + 1)}
            variant="ghost"
          >
            {t('nextPage')}
          </Button>
        </div>
      </Show>

      <Modal
        onClose={closeEdit}
        open={editing() !== null}
        title={t('editBook')}
      >
        <p class="text-muted mb-3 text-sm">#{editing()?.book_id ?? ''}</p>
        <label class="mb-3 flex flex-col gap-1 text-sm">
          <span class="text-muted">{t('titleLabel')}</span>
          <Input onInput={setEditTitle} value={editTitle()} />
        </label>
        <label class="mb-3 flex flex-col gap-1 text-sm">
          <span class="text-muted">{t('authorLabel')}</span>
          <Input onInput={setEditAuthor} value={editAuthor()} />
        </label>
        <div class="mb-4">
          <Toggle
            checked={editPublic()}
            label={t('isPublicLabel')}
            onChange={setEditPublic}
          />
        </div>
        <div class="flex justify-end gap-2">
          <Button disabled={saving()} onClick={closeEdit} variant="ghost">
            {t('cancel')}
          </Button>
          <Button
            disabled={saving() || !editTitle().trim()}
            onClick={() => void saveEdit()}
          >
            {saving() ? <Spinner class="size-4" /> : null}
            {t('save')}
          </Button>
        </div>
      </Modal>

      <ConfirmDialog
        message={t('deleteBookConfirm')}
        onClose={() => setDeleting(null)}
        onConfirm={() => void confirmDelete()}
        open={deleting() !== null}
        title={deleting()?.title ?? ''}
      />
    </SectionCard>
  );
};

const RowButton: Component<{ label: string; onClick: () => void }> = props => (
  <button
    class="border-edge text-muted hover:bg-page hover:text-ink cursor-pointer rounded border px-2 py-1 text-xs"
    onClick={() => {
      props.onClick();
    }}
    type="button"
  >
    {props.label}
  </button>
);
