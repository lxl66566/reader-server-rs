// Book detail page: parallel metadata, owner management and content tabs.
import { useNavigate, useParams } from '@solidjs/router';
import { VsDownload, VsEdit, VsTrash } from 'solid-icons/vs';
import { createResource, createSignal, Show, type Component } from 'solid-js';

import { bookApi, markerApi, readingApi } from '../api';
import type { Annotation, BookDetail, Bookmark } from '../api/types';
import { BookMeta } from '../components/book/BookMeta';
import { ConfirmModal } from '../components/book/ConfirmModal';
import { DetailTabs } from '../components/book/DetailTabs';
import {
  EditBookModal,
  type BookPatch,
} from '../components/book/EditBookModal';
import { EditReadingTimeModal } from '../components/book/EditReadingTimeModal';
import { TagEditor } from '../components/book/TagEditor';
import { LoadErrorPanel } from '../components/LoadErrorPanel';
import { IconButton, Spinner } from '../components/ui';
import { useI18n } from '../i18n';
import { saveResponseFile } from '../utils/download';
import { toastError } from '../utils/toast';

const BookDetailPage: Component = () => {
  const params = useParams();
  const navigate = useNavigate();

  // All four payloads load in parallel once the route param resolves.
  const bookId = () => Number(params.id);
  const [detail, { refetch: refetchDetail }] = createResource(bookId, id =>
    bookApi.detail(id),
  );
  const [bookmarks, { refetch: refetchBookmarks }] = createResource(
    bookId,
    id => markerApi.bookmarks(id),
  );
  const [annotations, { refetch: refetchAnnotations }] = createResource(
    bookId,
    id => markerApi.annotations(id),
  );
  const [tags, { refetch: refetchTags }] = createResource(bookId, id =>
    bookApi.tags(id),
  );

  const retry = () => {
    void refetchDetail();
    void refetchBookmarks();
    void refetchAnnotations();
    void refetchTags();
  };

  const removeBook = async () => {
    await bookApi.remove(bookId());
    navigate('/', { replace: true });
  };

  const updateBook = async (patch: BookPatch) => {
    await bookApi.update(bookId(), patch);
    void refetchDetail();
  };

  const removeBookmark = async (id: number) => {
    await markerApi.removeBookmark(bookId(), id);
    void refetchBookmarks();
  };

  const removeAnnotation = async (id: number) => {
    await markerApi.removeAnnotation(bookId(), id);
    void refetchAnnotations();
  };

  const setTags = async (next: string[]) => {
    await bookApi.setTags(bookId(), next);
    void refetchTags();
  };

  const setReadingTime = async (seconds: number) => {
    await readingApi.setReadingTime(bookId(), seconds);
    void refetchDetail();
  };

  // detail.error is `any` in SolidJS types; coerce before it reaches JSX.
  const hasError = () => Boolean(detail.error);
  const invalidId = () => Number.isNaN(bookId());

  return (
    <>
      <Show when={invalidId() || hasError()}>
        <LoadErrorPanel
          onBack={() => {
            navigate('/');
          }}
          onRetry={retry}
        />
      </Show>
      <Show when={!invalidId() && detail.loading}>
        <div class="flex justify-center py-16">
          <Spinner class="text-muted size-6" />
        </div>
      </Show>
      <Show when={!invalidId() && !detail.loading && detail()}>
        {/* detail() is guaranteed here by the Show guard above */}
        <BookView
          annotations={annotations()?.annotations ?? []}
          book={detail()!}
          bookmarks={bookmarks()?.bookmarks ?? []}
          onBookDelete={removeBook}
          onBookUpdate={updateBook}
          onDeleteAnnotation={removeAnnotation}
          onDeleteBookmark={removeBookmark}
          onSetReadingTime={setReadingTime}
          onTagsChange={setTags}
          tags={tags()?.tags ?? []}
        />
      </Show>
    </>
  );
};

// Loaded book content: meta header with owner icon controls, plus tabs.
function BookView(props: {
  annotations: Annotation[];
  book: BookDetail;
  bookmarks: Bookmark[];
  onBookDelete: () => Promise<void>;
  onBookUpdate: (patch: BookPatch) => Promise<void>;
  onDeleteAnnotation: (id: number) => Promise<void>;
  onDeleteBookmark: (id: number) => Promise<void>;
  onSetReadingTime: (seconds: number) => Promise<void>;
  onTagsChange: (tags: string[]) => Promise<void>;
  tags: string[];
}) {
  const { t } = useI18n();
  const navigate = useNavigate();
  const [editOpen, setEditOpen] = createSignal(false);
  const [deleteOpen, setDeleteOpen] = createSignal(false);
  const [readingTimeOpen, setReadingTimeOpen] = createSignal(false);
  const [downloading, setDownloading] = createSignal(false);

  const downloadBook = async () => {
    setDownloading(true);
    try {
      const res = await bookApi.downloadFile(props.book.book_id);
      // 服务端给出「书名 - 作者.ext」；此 fallback 仅在响应头缺失时使用
      const stem = props.book.author
        ? `${props.book.title} - ${props.book.author}`
        : props.book.title;
      await saveResponseFile(res, `${stem}.${props.book.format}`);
    } catch (err) {
      toastError(err, t);
    } finally {
      setDownloading(false);
    }
  };

  return (
    <div class="space-y-6">
      <BookMeta
        actions={
          <>
            <IconButton
              aria-label={t('downloadBook')}
              disabled={downloading()}
              onClick={() => {
                void downloadBook();
              }}
            >
              <Show fallback={<Spinner class="size-4" />} when={!downloading()}>
                <VsDownload class="size-4" />
              </Show>
            </IconButton>
            <Show when={props.book.is_owner}>
              <IconButton
                aria-label={t('editBook')}
                onClick={() => {
                  setEditOpen(true);
                }}
              >
                <VsEdit class="size-4" />
              </IconButton>
              <IconButton
                aria-label={t('delete')}
                onClick={() => {
                  setDeleteOpen(true);
                }}
                variant="danger"
              >
                <VsTrash class="size-4" />
              </IconButton>
            </Show>
          </>
        }
        book={props.book}
        onEditReadingTime={() => {
          setReadingTimeOpen(true);
        }}
        onRead={() => {
          navigate(`/read/${props.book.book_id}`);
        }}
      >
        <Show when={props.book.is_owner}>
          <TagEditor onChange={props.onTagsChange} tags={props.tags} />
        </Show>
      </BookMeta>

      <DetailTabs
        annotations={props.annotations}
        bookId={props.book.book_id}
        bookmarks={props.bookmarks}
        chapters={props.book.chapters}
        onDeleteAnnotation={props.onDeleteAnnotation}
        onDeleteBookmark={props.onDeleteBookmark}
      />

      <EditBookModal
        book={props.book}
        onClose={() => setEditOpen(false)}
        onSave={props.onBookUpdate}
        open={editOpen()}
      />
      <EditReadingTimeModal
        bookId={props.book.book_id}
        onClose={() => setReadingTimeOpen(false)}
        onSave={props.onSetReadingTime}
        open={readingTimeOpen()}
        seconds={props.book.reading_time}
      />
      <ConfirmModal
        message={t('deleteBookConfirm')}
        onClose={() => setDeleteOpen(false)}
        onConfirm={props.onBookDelete}
        open={deleteOpen()}
      />
    </div>
  );
}

export default BookDetailPage;
