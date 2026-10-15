// 回收站弹窗：列出已软删书籍，支持恢复与彻底删除。
// purge 成功后同步清理本地离线缓存（IndexedDB）。

import { VsCheck, VsDiscard } from 'solid-icons/vs';
import {
  createEffect,
  createResource,
  createSignal,
  For,
  Show,
  type Component,
} from 'solid-js';
import toast from 'solid-toast';

import { bookApi } from '../../api';
import type { TrashBook } from '../../api/types';
import { useI18n } from '../../i18n';
import type { DictKey } from '../../i18n/zh-CN';
import { clearBookContent } from '../../offline/db';
import { formatDateTime } from '../../utils/format';
import { toastError } from '../../utils/toast';
import { ConfirmModal } from '../book/ConfirmModal';
import { EmptyState, IconButton, Modal, Spinner } from '../ui';
import { Pagination } from './Pagination';

const TRASH_PAGE_SIZE = 5;

const FORMAT_LABELS: Record<TrashBook['format'], DictKey> = {
  epub: 'formatEpub',
  pdf: 'formatPdf',
  txt: 'formatTxt',
};

export const TrashModal: Component<{
  /** 恢复/彻底删除后通知父级刷新书架与公开书库 */
  onChanged: () => void;
  onClose: () => void;
  open: boolean;
}> = props => {
  const { locale, t } = useI18n();
  const [page, setPage] = createSignal(1);
  // 待彻底删除的书（确认框目标）
  const [purgeTarget, setPurgeTarget] = createSignal<TrashBook>();

  // 重新打开时回到第一页
  createEffect(() => {
    if (props.open) setPage(1);
  });

  const [trash, { refetch }] = createResource(
    () => ({ open: props.open, page: page() }),
    async key =>
      key.open ? bookApi.listTrash(key.page, TRASH_PAGE_SIZE) : undefined,
  );

  const totalPages = () =>
    Math.max(1, Math.ceil((trash()?.total ?? 0) / TRASH_PAGE_SIZE));

  const restore = async (bookId: number) => {
    try {
      await bookApi.restore(bookId);
      toast.success(t('restoredFromTrash'));
      await refetch();
      props.onChanged();
    } catch (err) {
      toastError(err, t);
    }
  };

  const purge = async () => {
    const target = purgeTarget();
    if (!target) return;
    try {
      await bookApi.purge(target.book_id);
      // 服务端数据已级联删除，本地离线缓存同步清理
      await clearBookContent(target.book_id);
      toast.success(t('purgedFromTrash'));
      setPurgeTarget();
      await refetch();
      props.onChanged();
    } catch (err) {
      toastError(err, t);
    }
  };

  return (
    <Modal onClose={props.onClose} open={props.open} title={t('trash')}>
      <Show
        fallback={
          <div class="flex justify-center py-8">
            <Spinner />
          </div>
        }
        when={!trash.loading || trash() !== undefined}
      >
        <Show
          fallback={<EmptyState text={t('trashEmpty')} />}
          when={(trash()?.books ?? []).length > 0}
        >
          <ul class="flex flex-col gap-2">
            <For each={trash()?.books ?? []}>
              {book => (
                <li class="border-edge flex items-center gap-2 rounded border px-3 py-2">
                  <div class="min-w-0 flex-1">
                    <p class="text-ink truncate text-sm">{book.title}</p>
                    <p class="text-muted truncate text-xs">
                      {book.author ?? t('unknownAuthor')} ·{' '}
                      {t(FORMAT_LABELS[book.format])} ·{' '}
                      {t('trashDeletedAt', {
                        date: formatDateTime(book.deleted_at, locale()),
                      })}
                    </p>
                  </div>
                  <IconButton
                    aria-label={t('trashRestore')}
                    disabled={trash.loading}
                    onClick={() => {
                      void restore(book.book_id);
                    }}
                  >
                    <VsCheck class="size-4" />
                  </IconButton>
                  <IconButton
                    aria-label={t('trashPurge')}
                    disabled={trash.loading}
                    onClick={() => {
                      setPurgeTarget(book);
                    }}
                    variant="danger"
                  >
                    <VsDiscard class="size-4" />
                  </IconButton>
                </li>
              )}
            </For>
          </ul>
          <Pagination
            disabled={trash.loading}
            onChange={setPage}
            page={page()}
            totalPages={totalPages()}
          />
        </Show>
      </Show>
      <ConfirmModal
        message={t('trashPurgeConfirm', { title: purgeTarget()?.title ?? '' })}
        onClose={() => {
          setPurgeTarget();
        }}
        onConfirm={purge}
        open={purgeTarget() !== undefined}
      />
    </Modal>
  );
};
