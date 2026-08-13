// Reader shell: owns the ws session, heartbeat loop, chrome and dispatches to
// the per-format reader. Format readers report positions; the shell heartbeats.

import { useParams, useSearchParams } from '@solidjs/router';
import {
  createEffect,
  createResource,
  createSignal,
  lazy,
  onCleanup,
  onMount,
  Show,
  Suspense,
} from 'solid-js';
import toast from 'solid-toast';

import { bookApi, markerApi } from '../api';
import type { BookDetail } from '../api/types';
import { Spinner } from '../components/ui';
import { useI18n } from '../i18n';
import Chrome from '../reader/Chrome';
import { createReadingSession } from '../reader/session';
import TxtReader from '../reader/TxtReader';
import type { ReaderNavigator } from '../reader/types';
import { useRequireAuth } from '../stores/auth';
import { useSettings } from '../stores/settings';

// heavy renderers load on demand
const EpubReader = lazy(() => import('../reader/EpubReader'));
const PdfReader = lazy(() => import('../reader/PdfReader'));

const HEARTBEAT_MS = 1000;
// no interaction for this long -> the user is not reading; pause time tracking
const ACTIVITY_WINDOW_MS = 60_000;

const Reader = () => {
  useRequireAuth();
  const params = useParams<{ id: string }>();
  const bookId = () => Number(params.id);

  const [book] = createResource(bookId, id => bookApi.detail(id));

  return (
    <Show
      fallback={
        <div class="flex h-dvh items-center justify-center">
          <Spinner />
        </div>
      }
      when={book()}
    >
      {detail => <ReaderBody book={detail()} />}
    </Show>
  );
};

export default Reader;

const ReaderBody = (props: { book: BookDetail }) => {
  const { t } = useI18n();
  const { settings } = useSettings();
  const [searchParams] = useSearchParams<{
    chapter?: string;
    locator?: string;
  }>();

  const [chromeVisible, setChromeVisible] = createSignal(false);
  const [reconnecting, setReconnecting] = createSignal(false);
  const [percent, setPercent] = createSignal(props.book.percent);
  const [annotationsVersion, setAnnotationsVersion] = createSignal(0);

  const [bookmarks, { refetch: refetchBookmarks }] = createResource(() =>
    markerApi.bookmarks(props.book.book_id).then(res => res.bookmarks),
  );
  const [annotations, { refetch: refetchAnnotations }] = createResource(() =>
    markerApi.annotations(props.book.book_id).then(res => res.annotations),
  );

  let navigatorRef: null | ReaderNavigator = null;
  let positionRef: null | { locator: string; percent: number } = null;

  const syncJump = (locator: string) => {
    navigatorRef?.jumpToLocator(locator);
    toast(t('readerSyncJump'), { icon: null });
  };

  const session = createReadingSession(props.book.book_id, {
    onAnnotationChanged: () => {
      setAnnotationsVersion(v => v + 1);
      void refetchAnnotations();
    },
    onBookmarkChanged: () => void refetchBookmarks(),
    onReconnecting: setReconnecting,
    onRemoteProgress: locator => {
      syncJump(locator);
    },
    onSyncRequired: locator => {
      syncJump(locator);
    },
  });

  // --- activity-aware heartbeat: only count time while the user is active ---
  let lastActivity = Date.now();
  const markActivity = () => {
    lastActivity = Date.now();
  };

  onMount(() => {
    // capture: the reader scrolls an inner container, window scroll never fires
    for (const event of ['pointerdown', 'touchstart', 'scroll', 'keydown']) {
      window.addEventListener(event, markActivity, {
        capture: true,
        passive: true,
      });
    }
    const timer = setInterval(() => {
      if (!positionRef) return;
      if (Date.now() - lastActivity > ACTIVITY_WINDOW_MS) return;
      session.send(positionRef.locator, positionRef.percent);
    }, HEARTBEAT_MS);
    onCleanup(() => {
      clearInterval(timer);
      for (const event of ['pointerdown', 'touchstart', 'scroll', 'keydown']) {
        window.removeEventListener(event, markActivity, { capture: true });
      }
    });
  });

  // deep links from the detail page: ?chapter=<id> or ?locator=<locator>
  onMount(() => {
    const chapterId = searchParams.chapter;
    const locator = searchParams.locator;
    if (chapterId) {
      void bookApi
        .jumpToChapter(props.book.book_id, Number(chapterId))
        .then(res => navigatorRef?.jumpToChapter(res.position))
        .catch(() => undefined);
    } else if (locator) {
      navigatorRef?.jumpToLocator(locator);
    }
  });

  const onPosition = (locator: string, nextPercent: number) => {
    positionRef = { locator, percent: nextPercent };
    setPercent(nextPercent);
  };

  // page uses the user's custom reading colors, overriding the global theme
  const surfaceStyle = () => ({
    background: settings.background_color,
    color: settings.text_color,
  });

  createEffect(() => {
    // keep the html background in sync so overscroll does not flash
    document.documentElement.style.background = settings.background_color;
    onCleanup(() => {
      document.documentElement.style.background = '';
    });
  });

  return (
    <div class="fixed inset-0 h-dvh w-full" style={surfaceStyle()}>
      <div class="absolute inset-0">
        <Suspense
          fallback={
            <div class="flex h-full items-center justify-center">
              <Spinner />
            </div>
          }
        >
          <Show when={props.book.format === 'txt'}>
            <TxtReader
              annotationsVersion={annotationsVersion}
              book={props.book}
              initialLocator={props.book.locator}
              onPosition={onPosition}
              onTapCenter={() => setChromeVisible(v => !v)}
              registerNavigator={nav => {
                navigatorRef = nav;
              }}
            />
          </Show>
          <Show when={props.book.format === 'epub'}>
            <EpubReader
              annotationsVersion={annotationsVersion}
              book={props.book}
              initialLocator={props.book.locator}
              onPosition={onPosition}
              onTapCenter={() => setChromeVisible(v => !v)}
              registerNavigator={nav => {
                navigatorRef = nav;
              }}
            />
          </Show>
          <Show when={props.book.format === 'pdf'}>
            <PdfReader
              annotationsVersion={annotationsVersion}
              book={props.book}
              initialLocator={props.book.locator}
              onPosition={onPosition}
              onTapCenter={() => setChromeVisible(v => !v)}
              registerNavigator={nav => {
                navigatorRef = nav;
              }}
            />
          </Show>
        </Suspense>
      </div>

      {/* dim layer closes the chrome on outside tap */}
      <Show when={chromeVisible()}>
        <div
          class="absolute inset-0 z-10"
          onClick={() => setChromeVisible(false)}
        />
      </Show>

      <Chrome
        annotations={annotations}
        book={props.book}
        bookmarks={bookmarks}
        currentPosition={() => positionRef}
        navigator={() => navigatorRef}
        percent={percent}
        reconnecting={reconnecting}
        refetchAnnotations={() => void refetchAnnotations()}
        refetchBookmarks={() => void refetchBookmarks()}
        visible={chromeVisible}
      />
    </div>
  );
};
