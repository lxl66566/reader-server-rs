// Home page: landing hero (logged-out) or library with search/upload/
// my books / recommendations / public library (logged-in).

import { useNavigate } from '@solidjs/router';
import {
  VsArrowDown,
  VsArrowUp,
  VsCloudUpload,
  VsRefresh,
  VsSearch,
  VsTrash,
} from 'solid-icons/vs';
import {
  createResource,
  createSignal,
  For,
  onCleanup,
  Show,
  type Component,
} from 'solid-js';

import { bookApi } from '../api';
import type {
  BookFormat,
  BookSortKey,
  SearchScope,
  SortOrder,
} from '../api/types';
import { BookCard } from '../components/home/BookCard';
import { BookSection } from '../components/home/BookSection';
import { Hero } from '../components/home/Hero';
import { Pagination } from '../components/home/Pagination';
import { TrashModal } from '../components/home/TrashModal';
import { UploadModal } from '../components/home/UploadModal';
import { Button, IconButton, Input } from '../components/ui';
import { useI18n } from '../i18n';
import type { DictKey } from '../i18n/zh-CN';
import { fetchBookList } from '../offline/api';
import { useAuth } from '../stores/auth';

const PAGE_SIZE = 20;
const SEARCH_DEBOUNCE_MS = 300;

// 排序/格式选项的 i18n 映射（Record 键序须为自然序）
const SORT_LABELS: Record<BookSortKey, DictKey> = {
  author: 'sortAuthor',
  created: 'sortCreated',
  last_read: 'sortLastRead',
  title: 'sortTitle',
};

const FORMAT_LABELS: Record<BookFormat, DictKey> = {
  epub: 'formatEpub',
  pdf: 'formatPdf',
  txt: 'formatTxt',
};

// select 选项语义序（非字母序）；解析用户输入用 find 回查，避免 as 断言
const SORT_OPTIONS: readonly BookSortKey[] = [
  'last_read',
  'created',
  'title',
  'author',
];
const FORMAT_OPTIONS: readonly BookFormat[] = ['txt', 'epub', 'pdf'];

const parseSortKey = (value: string): BookSortKey | undefined =>
  SORT_OPTIONS.find(key => key === value);

const parseFormat = (value: string): BookFormat | undefined =>
  FORMAT_OPTIONS.find(format => format === value);

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
  const [sort, setSort] = createSignal<BookSortKey>('last_read');
  const [order, setOrder] = createSignal<SortOrder>('desc');
  const [format, setFormat] = createSignal<BookFormat | undefined>(undefined);
  const [uploadOpen, setUploadOpen] = createSignal(false);
  const [trashOpen, setTrashOpen] = createSignal(false);

  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => {
    clearTimeout(searchTimer);
  });

  const [myBooks, { refetch: refetchMyBooks }] = createResource(
    () => [page(), activeQuery(), scope(), sort(), order(), format()] as const,
    key => {
      const [p, q, s, sortKey, sortOrder, fmt] = key;
      return q
        ? bookApi.search(q, s, p, PAGE_SIZE)
        : fetchBookList(p, PAGE_SIZE, {
            format: fmt,
            order: sortOrder,
            sort: sortKey,
          });
    },
  );

  // 搜索走独立 endpoint，排序/筛选不生效，此时禁用相关控件避免误导
  const searching = () => activeQuery() !== '';

  const [recommended, { refetch: refetchRecommended }] = createResource(() =>
    bookApi.randomPublic(3),
  );

  const [publicBooks, { refetch: refetchPublic }] = createResource(
    () => publicPage(),
    p => bookApi.listPublic(p, PAGE_SIZE),
  );

  // 提交搜索：立即生效并重置分页
  const commitQuery = () => {
    clearTimeout(searchTimer);
    setActiveQuery(searchQuery().trim());
    setPage(1);
  };

  // 动态搜索：输入停顿 SEARCH_DEBOUNCE_MS 后自动提交，避免每个按键都发请求
  const onSearchInput = (value: string) => {
    setSearchQuery(value);
    clearTimeout(searchTimer);
    searchTimer = setTimeout(commitQuery, SEARCH_DEBOUNCE_MS);
  };

  const submitSearch = (event: KeyboardEvent) => {
    if (event.key !== 'Enter') return;
    commitQuery();
  };

  // 切换排序键时方向复位为降序（语义上最自然的默认），并回到第一页
  const changeSort = (next: BookSortKey) => {
    setSort(next);
    setOrder('desc');
    setPage(1);
  };

  const changeFormat = (next: BookFormat | undefined) => {
    setFormat(next);
    setPage(1);
  };

  const toggleOrder = () => {
    setOrder(prev => (prev === 'desc' ? 'asc' : 'desc'));
    setPage(1);
  };

  const onUploaded = () => {
    setPage(1);
    void refetchMyBooks();
    void refetchPublic();
  };

  const goRead = (bookId: number) => {
    navigate(`/read/${bookId}`);
  };

  const goDetail = (bookId: number) => {
    navigate(`/book/${bookId}`);
  };

  return (
    <div class="flex flex-col gap-6">
      {/* 窄屏可折行：搜索框保底宽度，排序/筛选控件整体换行不溢出 */}
      <div class="flex flex-wrap items-center gap-2">
        <div class="relative min-w-44 flex-1">
          <VsSearch class="text-muted absolute top-1/2 left-2.5 size-4 -translate-y-1/2" />
          <Input
            class="w-full pl-8"
            onInput={onSearchInput}
            onKeyDown={submitSearch}
            placeholder={t('searchPlaceholder')}
            value={searchQuery()}
          />
        </div>
        <select
          aria-label={t('sortBy')}
          class="border-edge bg-surface text-ink focus:border-accent shrink-0 rounded border px-2 py-1.5 text-sm outline-none"
          disabled={searching()}
          onChange={event => {
            const next = parseSortKey(event.currentTarget.value);
            if (next) changeSort(next);
          }}
          value={sort()}
        >
          <For each={SORT_OPTIONS}>
            {key => <option value={key}>{t(SORT_LABELS[key])}</option>}
          </For>
        </select>
        <IconButton
          aria-label={t(order() === 'desc' ? 'sortDesc' : 'sortAsc')}
          disabled={searching()}
          onClick={toggleOrder}
        >
          <Show
            fallback={<VsArrowUp class="size-4" />}
            when={order() === 'desc'}
          >
            <VsArrowDown class="size-4" />
          </Show>
        </IconButton>
        <select
          aria-label={t('formatFilter')}
          class="border-edge bg-surface text-ink focus:border-accent shrink-0 rounded border px-2 py-1.5 text-sm outline-none"
          disabled={searching()}
          onChange={event => {
            changeFormat(parseFormat(event.currentTarget.value));
          }}
          value={format() ?? ''}
        >
          <option value="">{t('all')}</option>
          <For each={FORMAT_OPTIONS}>
            {formatKey => (
              <option value={formatKey}>{t(FORMAT_LABELS[formatKey])}</option>
            )}
          </For>
        </select>
        <select
          class="border-edge bg-surface text-ink focus:border-accent shrink-0 rounded border px-2 py-1.5 text-sm outline-none"
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
        actions={
          <Button
            class="px-2 text-xs"
            onClick={() => {
              setTrashOpen(true);
            }}
            variant="ghost"
          >
            <VsTrash class="size-4" />
            {t('trash')}
          </Button>
        }
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
                onDetail={() => {
                  goDetail(book.book_id);
                }}
                onRead={() => {
                  goRead(book.book_id);
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
                onDetail={() => {
                  goDetail(book.book_id);
                }}
                onRead={() => {
                  goRead(book.book_id);
                }}
                owner={book.owner_username}
                percent={book.percent ?? undefined}
                readingTime={book.reading_time ?? undefined}
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
                onDetail={() => {
                  goDetail(book.book_id);
                }}
                onRead={() => {
                  goRead(book.book_id);
                }}
                percent={book.percent ?? undefined}
                readingTime={book.reading_time ?? undefined}
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
      <TrashModal
        onChanged={() => {
          // 恢复的书回到书架；purge 公开书会影响公开书库
          void refetchMyBooks();
          void refetchPublic();
        }}
        onClose={() => setTrashOpen(false)}
        open={trashOpen()}
      />
    </div>
  );
};

function totalPages(total: number): number {
  return Math.max(1, Math.ceil(total / PAGE_SIZE));
}

export default Home;
