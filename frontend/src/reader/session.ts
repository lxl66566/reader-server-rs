// WebSocket reading session: subscribe + heartbeat channel with auto-reconnect.
// Owns the connection; the reader page owns the heartbeat timer and calls send().

import { onCleanup, onMount } from 'solid-js';
import toast from 'solid-toast';

import { ApiError, authApi, getDeviceId, getToken } from '../api';
import type { WsClientMessage, WsServerMessage } from '../api/types';
import { useI18n } from '../i18n';

export interface ReadingSession {
  close: () => void;
  connected: () => boolean;
  send: (locator: string, percent: number) => void;
}

export interface ReadingSessionHandlers {
  /** annotation list changed elsewhere; refetch */
  onAnnotationChanged: () => void;
  /** bookmark list changed elsewhere; refetch */
  onBookmarkChanged: () => void;
  onReconnecting?: (reconnecting: boolean) => void;
  /** another device moved; jump to stay in sync */
  onRemoteProgress: (locator: string, percent: number) => void;
  /** server says another device owns the progress; jump to its position */
  onSyncRequired: (locator: string, percent: number) => void;
}

const MAX_BACKOFF_MS = 10_000;
// 连续重连次数上限：超过后放弃并提示，避免无限重连、横幅永挂
const MAX_RECONNECT_ATTEMPTS = 10;

export function createReadingSession(
  bookId: number,
  handlers: ReadingSessionHandlers,
): ReadingSession {
  let ws: null | WebSocket = null;
  let closed = false;
  let backoff = 500;
  let attempts = 0;
  let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  const { t } = useI18n();

  const isOpen = () => ws !== null && ws.readyState === WebSocket.OPEN;

  const sendMessage = (message: WsClientMessage) => {
    if (isOpen()) ws?.send(JSON.stringify(message));
  };

  const scheduleReconnect = () => {
    attempts += 1;
    if (attempts > MAX_RECONNECT_ATTEMPTS) {
      // 放弃重连：摘掉横幅并提示用户，而不是无限重试
      handlers.onReconnecting?.(false);
      toast(t('readerReconnectFailed'), { icon: null });
      return;
    }
    handlers.onReconnecting?.(true);
    reconnectTimer = setTimeout(connect, backoff);
    backoff = Math.min(backoff * 2, MAX_BACKOFF_MS);
  };

  // 握手被拒时浏览器拿不到 HTTP 状态码：用一次 REST 请求甄别 token 是否已失效。
  // 若返回 401，全局 handler 会清理登录态（路由守卫随后跳转），
  // 这里停止重连并摘掉重连横幅。
  const onHandshakeRejected = async () => {
    try {
      await authApi.userInfo();
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) {
        closed = true;
        handlers.onReconnecting?.(false);
        return;
      }
    }
    if (!closed) scheduleReconnect();
  };

  const connect = () => {
    const token = getToken('user');
    if (!token || closed) return;
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    // 本次连接是否成功握手过：未握手即断开说明是握手被拒（如 token 过期）
    let opened = false;
    ws = new WebSocket(
      `${proto}://${location.host}/api/v1/reading/ws?token=${encodeURIComponent(token)}`,
    );

    ws.onopen = () => {
      opened = true;
      attempts = 0;
      backoff = 500;
      handlers.onReconnecting?.(false);
      sendMessage({
        book_id: bookId,
        device_id: getDeviceId(),
        type: 'subscribe',
      });
    };

    ws.onmessage = event => {
      const message = parseServerMessage(event.data);
      if (!message || message.type === 'error' || message.book_id !== bookId) {
        return;
      }
      switch (message.type) {
        case 'annotation_changed':
          handlers.onAnnotationChanged();
          break;
        case 'bookmark_changed':
          handlers.onBookmarkChanged();
          break;
        case 'heartbeat_ack':
          if (!message.synced) {
            handlers.onSyncRequired(message.locator, message.percent);
          }
          break;
        case 'progress_update':
          // ignore our own echo (server should exclude us, but be safe)
          if (message.device_id !== getDeviceId()) {
            handlers.onRemoteProgress(message.locator, message.percent);
          }
          break;
        default:
          break;
      }
    };

    ws.onclose = () => {
      ws = null;
      if (closed) return;
      if (opened) {
        // 已建立的连接断开：指数退避重连（有上限）
        scheduleReconnect();
      } else {
        // 握手即失败：先甄别是否鉴权问题再决定是否重连
        void onHandshakeRejected();
      }
    };

    ws.onerror = () => {
      // onclose follows; reconnect is handled there
    };
  };

  onMount(connect);

  onCleanup(() => {
    closed = true;
    clearTimeout(reconnectTimer);
    if (isOpen()) {
      sendMessage({ book_id: bookId, type: 'unsubscribe' });
    }
    ws?.close();
    ws = null;
  });

  return {
    close: () => {
      closed = true;
      ws?.close();
    },
    connected: isOpen,
    send: (locator, percent) => {
      sendMessage({ book_id: bookId, locator, percent, type: 'heartbeat' });
    },
  };
}

function parseServerMessage(data: unknown): null | WsServerMessage {
  if (typeof data !== 'string') return null;
  try {
    return JSON.parse(data) as WsServerMessage;
  } catch {
    return null;
  }
}
