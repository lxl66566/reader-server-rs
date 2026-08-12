// Home page: landing hero (logged-out) or library with search/upload/
// my books / recommendations / public library (logged-in).

import { useNavigate } from '@solidjs/router';
import { VsCloudUpload, VsRefresh, VsSearch } from 'solid-icons/vs';
import {
  createResource,
  createSignal,
  For,
  Show,
  type Component,
} from 'solid-js';
import { bookApi } from '../api';
import type { SearchScope } from '../api/types';
import { BookCard } from '../components/home/BookCard';
import { BookSection } from '../components/home/BookSection';
import { Hero } from '../components/home/Hero';
import { Pagination } from '../components/home/Pagination';
import { UploadModal } from '../components/home/UploadModal';
import { Button, Input } from '../components/ui';
import { useI18n } from '../i18n';
import { useAuth } from '../stores/auth';

const PAGE_SIZE = 20;

const Home: Component = () => {
  const auth = useAuth();
  return (
    <Show fallback={<Hero />} when={auth.isLoggedIn()}>
      <Library />
    </Show>
  );
};

const Library: Component = () => {
  const { t } = useI18n();
  const navigate = useNavigate();
  const [activeQuery, setActiveQuery] = createSignal('');
  const [page, setPage] = createSignal(1);
  const [publicPage, setPublicPage] = createSignal(1);
  const [scope, setScope] = createSignal<SearchScope>('mine');
  const [searchQuery, setSearchQuery] = createSignal('');
  const [uploadOpen, setUploadOpen] = createSignal(false);

  const [myBooks, { refetch: refetchMyBooks }] = createResource(
    () => [page(), activeQuery(), scope()] as const,
    key => {
      const [p, q, s] = key;
      return q
        ? bookApi.search(q, s, p, PAGE_SIZE)
        : bookApi.list(p, PAGE_SIZE);
    },
  );

  const [recommended, { refetch: refetchRecommended }] = createResource(() =>
    bookApi.randomPublic(3),
  );

  const [publicBooks, { refetch: refetchPublic }] = createResource(
    () => publicPage(),
    p => bookApi.listPublic(p, PAGE_SIZE),
  );

  const submitSearch = (event: KeyboardEvent) => {
    if (event.key !== 'Enter') return;
    setActiveQuery(searchQuery().trim());
    setPage(1);
  };

  const onUploaded = () => {
    setPage(1);
    void refetchMyBooks();
    void refetchPublic();
  };

  const goToBook = (bookId: number) => {
    navigate(`/book/${bookId}`);
  };

  return (
    <div class="flex flex-col gap-6">
      <div class="flex items-center gap-2">
        <div class="relative flex-1">
          <VsSearch class="absolute left-2.5 top-1/2 size-4 -translate-y-1/2 text-muted" />
          <Input
            class="w-full pl-8"
            onInput={setSearchQuery}
            onKeyDown={submitSearch}
            placeholder={t('searchPlaceholder')}
            value={searchQuery()}
          />
        </div>
        <select
          class="shrink-0 rounded border border-edge bg-surface px-2 py-1.5 text-sm text-ink outline-none focus:border-accent"
          onChange={event => {
            setScope(event.currentTarget.value as SearchScope);
          }}
          value={scope()}
        >
          <option value="mine">{t('searchScopeMine')}</option>
          <option value="public">{t('searchScopePublic')}</option>
          <option value="all">{t('searchScopeAll')}</option>
        </select>
        <Button class="shrink-0" onClick={() => setUploadOpen(true)}>
          <VsCloudUpload class="size-4" />
          <span class="hidden sm:inline">{t('uploadBook')}</span>
        </Button>
      </div>

      <BookSection
        empty={(myBooks()?.books ?? []).length === 0}
        emptyText={activeQuery() ? t('emptySearch') : t('emptyBooks')}
        error={myBooks.error !== undefined}
        hasData={myBooks() !== undefined}
        loading={myBooks.loading}
        onRetry={() => void refetchMyBooks()}
        title={t('myBooks')}
      >
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <For each={myBooks()?.books ?? []}>
            {book => (
              <BookCard
                author={book.author}
                format={book.format}
                isPublic={book.is_public}
                onClick={() => {
                  goToBook(book.book_id);
                }}
                percent={book.percent}
                readingTime={book.reading_time}
                title={book.title}
              />
            )}
          </For>
        </div>
        <Pagination
          disabled={myBooks.loading}
          onChange={setPage}
          page={page()}
          totalPages={totalPages(myBooks()?.total ?? 0)}
        />
      </BookSection>

      <BookSection
        actions={
          <Button
            class="px-2 text-xs"
            onClick={() => void refetchRecommended()}
            variant="ghost"
          >
            <VsRefresh class="size-4" />
            {t('recommendAnother')}
          </Button>
        }
        empty={(recommended()?.books ?? []).length === 0}
        emptyText={t('recommendEmpty')}
        error={recommended.error !== undefined}
        hasData={recommended() !== undefined}
        loading={recommended.loading}
        onRetry={() => void refetchRecommended()}
        title={t('recommendToMe')}
      >
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <For each={recommended()?.books ?? []}>
            {book => (
              <BookCard
                author={book.author}
                format={book.format}
                onClick={() => {
                  goToBook(book.book_id);
                }}
                owner={book.owner_username}
                title={book.title}
              />
            )}
          </For>
        </div>
      </BookSection>

      <BookSection
        empty={(publicBooks()?.books ?? []).length === 0}
        emptyText={t('emptyPublic')}
        error={publicBooks.error !== undefined}
        hasData={publicBooks() !== undefined}
        loading={publicBooks.loading}
        onRetry={() => void refetchPublic()}
        title={t('publicLibrary')}
      >
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <For each={publicBooks()?.books ?? []}>
            {book => (
              <BookCard
                author={book.author}
                format={book.format}
                onClick={() => {
                  goToBook(book.book_id);
                }}
                title={book.title}
              />
            )}
          </For>
        </div>
        <Pagination
          disabled={publicBooks.loading}
          onChange={setPublicPage}
          page={publicPage()}
          totalPages={totalPages(publicBooks()?.total ?? 0)}
        />
      </BookSection>

      <UploadModal
        onClose={() => setUploadOpen(false)}
        onUploaded={onUploaded}
        open={uploadOpen()}
      />
    </div>
  );
};

function totalPages(total: number): number {
  return Math.max(1, Math.ceil(total / PAGE_SIZE));
}

export default Home;
