// WebSocket reading session: subscribe + heartbeat channel with auto-reconnect.
// Owns the connection; the reader page owns the heartbeat timer and calls send().

import { onCleanup, onMount } from 'solid-js';

import { getDeviceId, getToken } from '../api';
import type { WsClientMessage, WsServerMessage } from '../api/types';

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

export function createReadingSession(
  bookId: number,
  handlers: ReadingSessionHandlers,
): ReadingSession {
  let ws: null | WebSocket = null;
  let closed = false;
  let backoff = 500;
  let reconnectTimer: ReturnType<typeof setTimeout> | undefined;

  const isOpen = () => ws !== null && ws.readyState === WebSocket.OPEN;

  const sendMessage = (message: WsClientMessage) => {
    if (isOpen()) ws?.send(JSON.stringify(message));
  };

  const connect = () => {
    const token = getToken('user');
    if (!token || closed) return;
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    ws = new WebSocket(
      `${proto}://${location.host}/api/v1/reading/ws?token=${encodeURIComponent(token)}`,
    );

    ws.onopen = () => {
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
      handlers.onReconnecting?.(true);
      reconnectTimer = setTimeout(connect, backoff);
      backoff = Math.min(backoff * 2, MAX_BACKOFF_MS);
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
