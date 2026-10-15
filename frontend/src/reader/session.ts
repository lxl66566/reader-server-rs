// WebSocket reading session: subscribe + heartbeat channel with auto-reconnect.
// Owns the connection; the reader page owns the heartbeat timer and calls send().

import { onCleanup, onMount } from 'solid-js';
import toast from 'solid-toast';

import { ApiError, authApi, getDeviceId, getToken } from '../api';
import type { WsClientMessage, WsServerMessage } from '../api/types';
import { useI18n } from '../i18n';
import { ReconnectScheduler } from './reconnectScheduler';

export interface ReadingSession {
  close: () => void;
  connected: () => boolean;
  /** 重连放弃后手动恢复；连接健康时调用无副作用 */
  retry: () => void;
  send: (locator: string, percent: number) => void;
}

export interface ReadingSessionHandlers {
  /** annotation list changed elsewhere; refetch */
  onAnnotationChanged: () => void;
  /** bookmark list changed elsewhere; refetch */
  onBookmarkChanged: () => void;
  /** 重连放弃（达到上限）：UI 应提供手动重连入口 */
  onGaveUp?: () => void;
  /** 每次心跳应答：synced=false 表示进度未落库（他端持有 / 新设备接管） */
  onHeartbeatAck?: (synced: boolean) => void;
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
  let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  const scheduler = new ReconnectScheduler(
    500,
    MAX_RECONNECT_ATTEMPTS,
    MAX_BACKOFF_MS,
  );
  const { t } = useI18n();

  const isOpen = () => ws !== null && ws.readyState === WebSocket.OPEN;

  const sendMessage = (message: WsClientMessage) => {
    if (isOpen()) ws?.send(JSON.stringify(message));
  };

  const clearReconnectTimer = () => {
    clearTimeout(reconnectTimer);
    reconnectTimer = undefined;
    scheduler.settle();
  };

  const scheduleReconnect = () => {
    const delay = scheduler.next();
    if (delay === null) {
      // 放弃自动重连：交由 UI 提供手动重连 / 网络恢复时自动重试
      handlers.onReconnecting?.(false);
      handlers.onGaveUp?.();
      toast(t('readerReconnectFailed'), { icon: null });
      return;
    }
    handlers.onReconnecting?.(true);
    reconnectTimer = setTimeout(connect, delay);
  };

  // 手动/网络恢复触发的重连：从干净退避状态重新开始
  const retry = () => {
    if (closed || isOpen() || !scheduler.isIdle()) return;
    scheduler.reset();
    handlers.onReconnecting?.(true);
    connect();
  };

  // 握手被拒时浏览器拿不到 HTTP 状态码：用一次 REST 请求甄别 token 是否已失效。
  // 若返回 401，全局 handler 会清理登录态（路由守卫随后跳转），
  // 这里停止重连并摘掉重连横幅。
  const onHandshakeRejected = async () => {
    try {
      await authApi.userInfo();
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) {
        teardown();
        handlers.onReconnecting?.(false);
        return;
      }
    }
    if (!closed) scheduleReconnect();
  };

  const connect = () => {
    // 已触发的定时重连不再算在途：哨兵不复位会永久拦截 retry / online 恢复
    clearReconnectTimer();
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
      scheduler.reset();
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
          handlers.onHeartbeatAck?.(message.synced);
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

  // 网络恢复时自动重试（放弃状态最常见的恢复路径）
  const onOnline = () => {
    if (scheduler.isExhausted()) retry();
  };
  window.addEventListener('online', onOnline);

  // 停止会话：停止重连、摘掉 online 监听并断开连接（组件卸载与
  // 握手被拒/手动 close 共用，可重复调用）
  const teardown = () => {
    closed = true;
    clearReconnectTimer();
    window.removeEventListener('online', onOnline);
    if (isOpen()) {
      sendMessage({ book_id: bookId, type: 'unsubscribe' });
    }
    ws?.close();
    ws = null;
  };

  onCleanup(teardown);

  return {
    close: teardown,
    connected: isOpen,
    retry,
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
