// Reader shell: owns the ws session, heartbeat loop, chrome and dispatches to
// the per-format reader. Format readers report positions; the shell heartbeats.

import { useNavigate, useParams, useSearchParams } from '@solidjs/router';
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

import type { BookDetail } from '../api/types';
import { LoadErrorPanel } from '../components/LoadErrorPanel';
import { Spinner } from '../components/ui';
import { useI18n } from '../i18n';
import {
  fetchAnnotations,
  fetchBookmarks,
  fetchDetail,
  markProgressPushed,
  readProgress,
  saveProgress,
} from '../offline/api';
import Chrome from '../reader/Chrome';
import { createReadingSession } from '../reader/session';
import TxtReader from '../reader/TxtReader';
import type { DeepLink, ReaderNavigator } from '../reader/types';
import { useRequireAuth } from '../stores/auth';
import { useSettings } from '../stores/settings';

// heavy renderers load on demand
const EpubReader = lazy(() => import('../reader/EpubReader'));
const PdfReader = lazy(() => import('../reader/PdfReader'));

const HEARTBEAT_MS = 1000;
// no interaction for this long -> the user is not reading; pause time tracking
const ACTIVITY_WINDOW_MS = 60_000;
// 远端进度应用的节流间隔与距离阈值：双端同读时避免每秒互跳 + toast 轰炸
const SYNC_JUMP_MIN_INTERVAL_MS = 2000;
const SYNC_JUMP_MIN_PERCENT_DELTA = 0.005;
const SYNC_TOAST_MIN_INTERVAL_MS = 10_000;
// 本地进度持久化的节流间隔：离线续读精度与 IDB 写频的折中
const PROGRESS_SAVE_MS = 3000;

const Reader = () => {
  useRequireAuth();
  const params = useParams<{ id: string }>();
  const navigate = useNavigate();
  const bookId = () => Number(params.id);
  // 路由参数非法时跳过请求，直接进错误态
  const invalidId = () => Number.isNaN(bookId());

  const [book, { refetch }] = createResource(
    () => (invalidId() ? null : bookId()),
    async (id: number) => {
      // fetchDetail：断网回退缓存详情；readProgress：本地进度恢复。
      // 详情来自缓存回退（等效离线）或进度未推送时，本地进度优先于
      // 缓存详情里的陈旧服务端进度
      const { detail, fromCache } = await fetchDetail(id);
      const cached = await readProgress(id);
      const start = cached && (cached.unpushed || fromCache) ? cached : null;
      return { detail, start };
    },
  );
  const hasError = () => invalidId() || Boolean(book.error);

  return (
    <Show
      fallback={
        <Show
          fallback={
            <div class="flex h-dvh items-center justify-center">
              <Spinner />
            </div>
          }
          when={hasError()}
        >
          <div class="flex h-dvh items-center justify-center">
            <LoadErrorPanel
              onBack={() => {
                navigate('/');
              }}
              onRetry={() => {
                void refetch();
              }}
            />
          </div>
        </Show>
      }
      when={book()}
    >
      {boot => <ReaderBody book={boot().detail} offlineStart={boot().start} />}
    </Show>
  );
};

export default Reader;

interface ReaderBodyProps {
  book: BookDetail;
  /** 本地未推送的离线进度：作为初始位置并抑制一次服务端回跳 */
  offlineStart: null | { locator: string; percent: number };
}

const ReaderBody = (props: ReaderBodyProps) => {
  const { t } = useI18n();
  const { reload: reloadSettings, settings } = useSettings();
  const [searchParams] = useSearchParams<{
    chapter?: string;
    locator?: string;
  }>();

  const [chromeVisible, setChromeVisible] = createSignal(false);
  const [reconnecting, setReconnecting] = createSignal(false);
  // 重连放弃状态：横幅变为可点击的手动重连入口
  const [wsGaveUp, setWsGaveUp] = createSignal(false);
  // 离线进度优先于服务端进度作为起点；percent 显示同步采用
  const [percent, setPercent] = createSignal(
    props.offlineStart ? props.offlineStart.percent : props.book.percent,
  );
  const initialLocator = props.offlineStart
    ? props.offlineStart.locator
    : props.book.locator;
  const [annotationsVersion, setAnnotationsVersion] = createSignal(0);

  // 断网回退缓存的书签/批注列表（写穿）
  const [bookmarks, { refetch: refetchBookmarks }] = createResource(() =>
    fetchBookmarks(props.book.book_id),
  );
  const [annotations, { refetch: refetchAnnotations }] = createResource(() =>
    fetchAnnotations(props.book.book_id),
  );

  // navigator 用 signal 而非普通 let：epub/pdf 的 reader 是 lazy 组件，
  // 注册晚于 Chrome 首次渲染；响应式引用保证目录面板等消费方能感知注册完成
  const [navigatorRef, setNavigatorRef] = createSignal<null | ReaderNavigator>(
    null,
  );
  let positionRef: null | { locator: string; percent: number } = null;

  // --- 本地进度持久化（离线续读） ---
  // 心跳 ack synced=true 表示当前位置已落库：清除回跳抑制并标记已推送
  let suppressSync = props.offlineStart !== null;
  let progressDirty = true;
  let lastProgressSave = 0;
  let pendingPosition: null | { locator: string; percent: number } = null;
  const saveProgressThrottled = (pos: { locator: string; percent: number }) => {
    pendingPosition = pos;
    const now = Date.now();
    if (now - lastProgressSave < PROGRESS_SAVE_MS) return;
    lastProgressSave = now;
    progressDirty = true;
    void saveProgress(props.book.book_id, pos.locator, pos.percent);
  };

  // --- 跨设备进度同步 ---
  // 跳转是异步的（txt 需加载窗口、epub 需加载章节）：期间清空 positionRef
  // 挂起心跳回写，避免旧位置被写入服务器再广播出去形成多端乒乓。
  let syncJumpSeq = 0; // 代际：旧跳转的收尾不得覆盖新跳转期间的状态
  let remoteJumping = false; // 当前 onPosition 是否源于远端同步跳转
  // 跳转期间到达的位置上报暂存于此：epub 的 relocated 会在 display 过程中
  // 陆续触发，直接回填 positionRef 会把章节起始等中间位置当最新进度写回
  // 服务端；跳转完成后取最后一次上报（无则恢复跳转前位置）回填
  let deferredPosition: null | { locator: string; percent: number } = null;
  let lastSyncJumpAt = 0;
  let lastSyncToastAt = 0;

  const syncJump = (locator: string): void => {
    const nav = navigatorRef();
    if (!nav) return;
    const seq = ++syncJumpSeq;
    const prev = positionRef;
    deferredPosition = null;
    positionRef = null;
    remoteJumping = true;
    void nav.jumpToLocator(locator).then(() => {
      if (seq !== syncJumpSeq) return;
      remoteJumping = false;
      // 跳转失败：仍停留在原位。无论成败都以上报过的实际位置优先
      // （无上报时恢复 prev，保证心跳不会无限挂起）
      positionRef = deferredPosition ?? prev;
      if (deferredPosition) setPercent(deferredPosition.percent);
    });
  };

  // 远端进度（他端更新 / 设备让位）统一入口：节流 + 距离阈值 + toast 去重。
  // 位置已收敛时直接跳过——服务端在让位时已把设备归属交给本机，
  // 不跳转也不会丢失写入权。
  const applyRemoteProgress = (
    locator: string,
    remotePercent: number,
  ): void => {
    // 本地未推送进度优先：抑制回跳，下一次心跳把本地进度写回服务端
    if (suppressSync) return;
    const now = Date.now();
    if (
      positionRef !== null &&
      Math.abs(remotePercent - positionRef.percent) <
        SYNC_JUMP_MIN_PERCENT_DELTA
    ) {
      return;
    }
    if (now - lastSyncJumpAt < SYNC_JUMP_MIN_INTERVAL_MS) return;
    lastSyncJumpAt = now;
    syncJump(locator);
    if (now - lastSyncToastAt > SYNC_TOAST_MIN_INTERVAL_MS) {
      lastSyncToastAt = now;
      toast(t('readerSyncJump'), { icon: null });
    }
  };

  // 批注变更的统一入口：本端写入与 ws 推送都走这里刷新列表并通知 reader 重绘
  const bumpAnnotations = () => {
    setAnnotationsVersion(v => v + 1);
    void refetchAnnotations();
  };

  const session = createReadingSession(props.book.book_id, {
    onAnnotationChanged: bumpAnnotations,
    onBookmarkChanged: () => void refetchBookmarks(),
    onGaveUp: () => setWsGaveUp(true),
    onHeartbeatAck: (_synced, updated) => {
      // 仅 updated=true 表示本次上报真正写入服务端：解除回跳抑制并标记已
      // 推送。竞争失败（updated=false）时保留本地 unpushed 进度，下一次
      // 心跳重推；让位（synced=false）的回跳由 onSyncRequired 处理
      if (!updated) return;
      suppressSync = false;
      if (progressDirty) {
        progressDirty = false;
        void markProgressPushed(props.book.book_id);
      }
    },
    onReconnecting: reconnecting => {
      setReconnecting(reconnecting);
      if (!reconnecting) setWsGaveUp(false);
    },
    onRemoteProgress: applyRemoteProgress,
    onSyncRequired: applyRemoteProgress,
  });

  // --- activity-aware heartbeat: only count time while the user is active ---
  let lastActivity = Date.now();
  const markActivity = () => {
    lastActivity = Date.now();
  };

  onMount(() => {
    // 拉取一次最新设置：settings store 仅在登录态变化时请求，
    // 同账号其他设备改过的设置本会话内否则不可见
    reloadSettings();
    // capture: the reader scrolls an inner container, window scroll never fires
    for (const event of ['pointerdown', 'touchstart', 'scroll', 'keydown']) {
      window.addEventListener(event, markActivity, {
        capture: true,
        passive: true,
      });
    }
    const timer = setInterval(() => {
      // positionRef 为空说明正在同步跳转或进度未知：心跳整体跳过
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

  // 详情页深链（?chapter=<id> / ?locator=<locator>）：作为 props 传给各
  // Reader 由其就绪后消费。不能在 onMount 里经 navigatorRef 分发——
  // EpubReader/PdfReader 是 lazy 加载，注册 navigator 时 onMount 早已跑完。
  const deepLink = (): DeepLink | null => {
    const chapterId = Number.parseInt(searchParams.chapter ?? '', 10);
    if (Number.isFinite(chapterId)) return { chapterId, kind: 'chapter' };
    const locator = searchParams.locator;
    return locator ? { kind: 'locator', locator } : null;
  };

  const onPosition = (locator: string, nextPercent: number) => {
    // 远端同步跳转进行中：暂存而非回填，避免 epub relocated 在 display
    // 过程中报出的中间位置被心跳写入服务端（见 syncJump 注释）
    if (remoteJumping) {
      deferredPosition = { locator, percent: nextPercent };
      setPercent(nextPercent);
      return;
    }
    positionRef = { locator, percent: nextPercent };
    setPercent(nextPercent);
    // 本地持久化：离线续读与重开页面时的进度恢复
    saveProgressThrottled(positionRef);
    // 位置变化是活跃证据（如 epub 分页翻页）；远端同步跳转除外
    markActivity();
  };

  // 卸载时补写最后一次节流未落盘的进度（离线场景尤其重要）
  onCleanup(() => {
    if (pendingPosition) {
      void saveProgress(
        props.book.book_id,
        pendingPosition.locator,
        pendingPosition.percent,
      );
    }
  });

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
              deepLink={deepLink()}
              initialLocator={initialLocator}
              onAnnotationsChanged={bumpAnnotations}
              onPosition={onPosition}
              onTapCenter={() => setChromeVisible(v => !v)}
              registerNavigator={setNavigatorRef}
            />
          </Show>
          <Show when={props.book.format === 'epub'}>
            <EpubReader
              annotationsVersion={annotationsVersion}
              book={props.book}
              deepLink={deepLink()}
              initialLocator={initialLocator}
              onActivity={markActivity}
              onPosition={onPosition}
              onTapCenter={() => setChromeVisible(v => !v)}
              registerNavigator={setNavigatorRef}
            />
          </Show>
          <Show when={props.book.format === 'pdf'}>
            <PdfReader
              annotationsVersion={annotationsVersion}
              book={props.book}
              deepLink={deepLink()}
              initialLocator={initialLocator}
              onAnnotationsChanged={bumpAnnotations}
              onPosition={onPosition}
              onTapCenter={() => setChromeVisible(v => !v)}
              registerNavigator={setNavigatorRef}
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
        navigator={navigatorRef}
        onRetryReconnect={() => {
          session.retry();
        }}
        percent={percent}
        reconnecting={reconnecting}
        refetchAnnotations={() => void refetchAnnotations()}
        refetchBookmarks={() => void refetchBookmarks()}
        visible={chromeVisible}
        wsGaveUp={wsGaveUp}
      />
    </div>
  );
};
