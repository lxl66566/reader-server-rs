// In-book full-text search panel (txt only). Input + result list; a hit jumps
// via the reader navigator's locator jump (serial chain, same as toc/bookmarks).

import { VsSearch } from 'solid-icons/vs';
import { createEffect, createSignal, For, Show } from 'solid-js';

import { bookApi } from '../api';
import type { BookDetail, BookSearchMatch } from '../api/types';
import {
  Button,
  EmptyState,
  IconButton,
  Input,
  Modal,
  Spinner,
} from '../components/ui';
import { useI18n } from '../i18n';
import { splitExcerpt } from './searchExcerpt';

type SearchStatus = 'done' | 'error' | 'idle' | 'loading';

const SearchPanel = (panelProps: {
  book: BookDetail;
  onClose: () => void;
  onJump: (charOffset: number) => void;
  open: boolean;
}) => {
  const { t } = useI18n();
  const [query, setQuery] = createSignal('');
  const [status, setStatus] = createSignal<SearchStatus>('idle');
  const [matches, setMatches] = createSignal<BookSearchMatch[]>([]);
  const [truncated, setTruncated] = createSignal(false);

  // 打开面板时聚焦输入框（Input 不转发 ref，借容器查询定位）
  let box: HTMLDivElement | undefined;
  createEffect(() => {
    if (panelProps.open) box?.querySelector('input')?.focus();
  });

  const search = async () => {
    const q = query().trim();
    // 空 q 由客户端拦截（后端同语义 400）
    if (!q || status() === 'loading') return;
    setStatus('loading');
    try {
      const res = await bookApi.searchInBook(panelProps.book.book_id, q);
      setMatches(res.matches);
      setTruncated(res.truncated);
      setStatus('done');
    } catch {
      // 失败给出可重试的错误态，不静默
      setStatus('error');
    }
  };

  const hitLen = () => Array.from(query().trim()).length;

  return (
    <Modal
      onClose={panelProps.onClose}
      open={panelProps.open}
      title={t('readerSearch')}
    >
      <div class="flex gap-2" ref={box}>
        <Input
          class="flex-1"
          onInput={setQuery}
          onKeyDown={event => {
            if (event.key === 'Enter') void search();
          }}
          placeholder={t('readerSearchPlaceholder')}
          value={query()}
        />
        <IconButton
          aria-label={t('readerSearch')}
          disabled={!query().trim() || status() === 'loading'}
          onClick={() => void search()}
        >
          <VsSearch class="size-4" />
        </IconButton>
      </div>

      <Show when={status() === 'error'}>
        {/* 失败给出可重试的错误态，不静默 */}
        <div class="flex flex-col items-center gap-3 py-6">
          <p class="text-muted text-sm">{t('errorLoadFailed')}</p>
          <Button onClick={() => void search()} variant="ghost">
            {t('retry')}
          </Button>
        </div>
      </Show>
      <Show when={status() === 'loading'}>
        <div class="flex justify-center py-6">
          <Spinner />
        </div>
      </Show>
      <Show when={status() === 'done'}>
        <Show
          fallback={<EmptyState text={t('readerSearchNoResults')} />}
          when={matches().length > 0}
        >
          <div class="text-muted mt-3 mb-1 text-xs">
            {t('readerSearchResultCount', { count: matches().length })}
            <Show when={truncated()}>
              <span> · {t('readerSearchTruncated')}</span>
            </Show>
          </div>
          <ul class="max-h-[55vh] overflow-y-auto">
            <For each={matches()}>
              {match => (
                <li>
                  <button
                    class="hover:bg-page w-full cursor-pointer rounded px-2 py-2 text-left"
                    onClick={() => {
                      panelProps.onJump(match.char_offset);
                    }}
                    type="button"
                  >
                    <span class="text-muted block truncate text-xs">
                      {match.chapter_title}
                    </span>
                    <span class="block text-sm break-all">
                      <Excerpt
                        excerpt={match.excerpt}
                        hitLen={hitLen()}
                        offset={match.char_offset}
                      />
                    </span>
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </Modal>
  );
};

// excerpt 三段渲染，命中段加粗（font-medium + 前景色突出）
const Excerpt = (excerptProps: {
  excerpt: string;
  hitLen: number;
  offset: number;
}) => {
  const parts = () =>
    splitExcerpt(
      excerptProps.excerpt,
      excerptProps.offset,
      excerptProps.hitLen,
    );
  return (
    <>
      <span class="text-muted">{parts().before}</span>
      <span class="text-ink font-medium">{parts().hit}</span>
      <span class="text-muted">{parts().after}</span>
    </>
  );
};

export default SearchPanel;
