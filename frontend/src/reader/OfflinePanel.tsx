// 离线缓存面板：缓存后续 N 章 / 缓存全本 / 清除缓存，含下载进度与取消。

import { VsCloudDownload } from 'solid-icons/vs';
import {
  createMemo,
  createResource,
  createSignal,
  For,
  Show,
  type Component,
} from 'solid-js';
import toast from 'solid-toast';

import type { BookDetail } from '../api/types';
import { Button, Modal, Spinner } from '../components/ui';
import { useI18n } from '../i18n';
import {
  chapterSpans,
  coverageOf,
  rangeFromChapter,
} from '../offline/coverage';
import { chunkPositions, fileBytes } from '../offline/db';
import {
  clearBook,
  DownloadCancelled,
  downloadBookFile,
  downloadTxtRange,
  saveDetail,
} from '../offline/download';
import { isQuotaError } from '../offline/net';
import { chapterIndexAt } from './textSource';

/** 字节数的人类可读展示 */
const formatBytes = (bytes: number): string =>
  bytes >= 1048_576
    ? `${(bytes / 1048_576).toFixed(1)} MB`
    : `${Math.max(1, Math.round(bytes / 1024))} KB`;

/** 可选的"缓存后续章节数" */
const CHAPTER_COUNTS = [10, 50, 100, 200] as const;

type DownloadState =
  | { done: number; kind: 'txt'; total: number }
  | { kind: 'file'; received: number }
  | { kind: 'idle' };

type OfflineStatus =
  | { bytes: null | number; kind: 'file' }
  | { covered: number; kind: 'txt'; total: number };

const OfflinePanel: Component<{
  book: BookDetail;
  currentPosition: () => null | { locator: string; percent: number };
  onClose: () => void;
  open: boolean;
}> = props => {
  const { t } = useI18n();
  const [chapterCount, setChapterCount] = createSignal<number>(50);
  const [download, setDownload] = createSignal<DownloadState>({ kind: 'idle' });
  let aborter: AbortController | null = null;

  const [status, { refetch }] = createResource(
    async (): Promise<OfflineStatus> => {
      if (props.book.format === 'txt') {
        const spans = chapterSpans(props.book.chapters, props.book.total_chars);
        const positions = await chunkPositions(props.book.book_id);
        return {
          covered: coverageOf(spans, positions).covered.length,
          kind: 'txt',
          total: spans.length,
        };
      }
      return { bytes: await fileBytes(props.book.book_id), kind: 'file' };
    },
  );

  const busy = () => download().kind !== 'idle';
  const hasChapters = () =>
    props.book.format === 'txt' && props.book.chapters.length > 0;

  const statusText = createMemo((): string => {
    const s = status();
    if (!s) return '';
    if (s.kind === 'txt') {
      return t('offlineCachedChapters', {
        cached: s.covered,
        total: s.total,
      });
    }
    return s.bytes === null
      ? t('offlineNotCached')
      : t('offlineFileCached', { size: formatBytes(s.bytes) });
  });

  const progressText = createMemo((): string => {
    const d = download();
    if (d.kind === 'txt') {
      return t('offlineProgressCount', { done: d.done, total: d.total });
    }
    if (d.kind === 'file') {
      return t('offlineProgressBytes', { size: formatBytes(d.received) });
    }
    return '';
  });

  const progressWidth = createMemo((): string => {
    const d = download();
    return d.kind === 'txt' && d.total > 0
      ? `${Math.round((d.done / d.total) * 100)}%`
      : '100%';
  });

  /** 当前章下标（txt locator 为字符偏移）；无进度时从首章起缓存 */
  const currentChapter = (): number => {
    const pos = props.currentPosition();
    const at = pos ? Number.parseInt(pos.locator, 10) : 0;
    return chapterIndexAt(props.book.chapters, Number.isFinite(at) ? at : 0);
  };

  const finish = async () => {
    await saveDetail(props.book);
    setDownload({ kind: 'idle' });
    void refetch();
  };

  const fail = (err: unknown) => {
    setDownload({ kind: 'idle' });
    if (err instanceof DownloadCancelled) return;
    if (isQuotaError(err)) {
      toast.error(t('offlineQuota'));
      return;
    }
    console.error('offline download failed', err);
    toast.error(t('errorGeneric'));
  };

  const cacheChapters = async () => {
    const range = rangeFromChapter(
      props.book.chapters,
      currentChapter(),
      chapterCount(),
      props.book.total_chars,
    );
    if (!range) return;
    aborter = new AbortController();
    setDownload({ done: 0, kind: 'txt', total: 0 });
    try {
      await downloadTxtRange(
        props.book.book_id,
        range.start,
        range.end,
        props.book.total_chars,
        aborter.signal,
        progress => setDownload({ kind: 'txt', ...progress }),
      );
      await finish();
      toast.success(t('offlineCacheDone'));
    } catch (err) {
      fail(err);
    }
  };

  const cacheAll = async () => {
    aborter = new AbortController();
    try {
      if (props.book.format === 'txt') {
        setDownload({ done: 0, kind: 'txt', total: 0 });
        await downloadTxtRange(
          props.book.book_id,
          0,
          props.book.total_chars,
          props.book.total_chars,
          aborter.signal,
          progress => setDownload({ kind: 'txt', ...progress }),
        );
      } else {
        setDownload({ kind: 'file', received: 0 });
        await downloadBookFile(
          props.book,
          received => setDownload({ kind: 'file', received }),
          aborter.signal,
        );
      }
      await finish();
      toast.success(t('offlineCacheDone'));
    } catch (err) {
      fail(err);
    }
  };

  const clear = async () => {
    await clearBook(props.book.book_id);
    void refetch();
  };

  return (
    <Modal onClose={props.onClose} open={props.open} title={t('offlineCache')}>
      <Show
        fallback={
          <div class="flex justify-center py-8">
            <Spinner />
          </div>
        }
        when={!status.loading && status()}
      >
        <div class="space-y-4">
          <p class="text-muted text-sm">{statusText()}</p>

          {/* 下载进度 */}
          <Show when={busy()}>
            <div class="space-y-2">
              <div class="bg-page h-1.5 w-full overflow-hidden rounded-full">
                <div
                  class="bg-accent h-full rounded-full transition-[width] duration-150"
                  style={{ width: progressWidth() }}
                />
              </div>
              <p class="text-muted text-center text-xs tabular-nums">
                {progressText()}
              </p>
            </div>
          </Show>

          {/* 操作区 */}
          <div class="flex flex-wrap items-center gap-2">
            <Show when={!busy()}>
              <Show when={hasChapters()}>
                <select
                  class="border-edge bg-surface text-ink focus:border-accent rounded border px-2 py-1.5 text-sm outline-none"
                  onChange={event =>
                    setChapterCount(Number(event.currentTarget.value))
                  }
                  value={chapterCount()}
                >
                  <For each={[...CHAPTER_COUNTS]}>
                    {count => <option value={count}>{count}</option>}
                  </For>
                </select>
                <Button onClick={() => void cacheChapters()}>
                  <VsCloudDownload class="size-4" />
                  {t('offlineCacheNext')}
                </Button>
              </Show>
              <Button onClick={() => void cacheAll()}>
                <VsCloudDownload class="size-4" />
                {t('offlineCacheAll')}
              </Button>
              <Button onClick={() => void clear()} variant="danger">
                {t('offlineClear')}
              </Button>
            </Show>
            <Show when={busy()}>
              <Button onClick={() => aborter?.abort()} variant="ghost">
                {t('cancel')}
              </Button>
            </Show>
          </div>

          <p class="text-muted text-xs">{t('offlineHint')}</p>
        </div>
      </Show>
    </Modal>
  );
};

export default OfflinePanel;
