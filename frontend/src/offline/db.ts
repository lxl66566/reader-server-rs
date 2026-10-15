// IndexedDB 持久缓存：离线阅读的书籍内容与元数据。
// 所有 key 以用户 id 为前缀隔离（JWT sub），避免共用浏览器时读到他人书籍。

import { openDB, type DBSchema, type IDBPDatabase } from 'idb';

import { getToken } from '../api/client';
import type { BookDetail } from '../api/types';

export interface CachedFile {
  data: ArrayBuffer;
  rev: string;
}

export interface CachedMeta {
  detail: BookDetail;
  /** 保存时间戳（ms），仅用于展示与调试 */
  savedAt: number;
}

/** 服务端 /content 的分块响应体 */
export interface ChunkPayload {
  content: string;
  next_position: number;
}

interface OfflineDB extends DBSchema {
  /** txt 分块内容（key 含对齐的 chunk 起始偏移） */
  chunks: { key: string; value: ChunkPayload };
  /** epub/pdf 整本文件（仅显式缓存时写入） */
  files: { key: string; value: CachedFile };
  /** 小对象：阅读设置等 */
  kv: { key: string; value: unknown };
  /** 书籍详情 */
  meta: { key: string; value: CachedMeta };
}

const DB_NAME = 'reader-offline';
const DB_VERSION = 1;

let dbPromise: null | Promise<IDBPDatabase<OfflineDB>> = null;

/** 书籍版本指纹：统计特征变化（重传/重切章）时旧缓存作废 */
export function bookRev(detail: BookDetail): string {
  return [
    detail.format,
    detail.total_chars,
    detail.total_pages,
    detail.chapters.length,
    detail.chapter_pattern ?? '',
  ].join('|');
}

/** 以当前用户为前缀拼 cache key */
export function cacheKey(...parts: (number | string)[]): string {
  return [userId(), ...parts].join(':');
}

export async function chunkGet(
  bookId: number,
  position: number,
): Promise<ChunkPayload | null> {
  try {
    return (
      (await (await db()).get('chunks', cacheKey('c', bookId, position))) ??
      null
    );
  } catch (err) {
    console.warn('offline chunk read failed', err);
    return null;
  }
}

/** 已缓存分块的起始偏移集合（升序） */
export async function chunkPositions(bookId: number): Promise<number[]> {
  try {
    const prefix = cacheKey('c', bookId) + ':';
    const keys = await (await db()).getAllKeys('chunks');
    const out: number[] = [];
    for (const key of keys) {
      if (!key.startsWith(prefix)) continue;
      const position = Number(key.slice(prefix.length));
      // key 由 chunkStart 对齐生成；数值化失败（NaN）说明是脏数据，跳过
      if (Number.isFinite(position)) out.push(position);
    }
    return out.toSorted((a, b) => a - b);
  } catch (err) {
    console.warn('offline chunk list failed', err);
    return [];
  }
}

export async function chunkPut(
  bookId: number,
  position: number,
  payload: ChunkPayload,
): Promise<void> {
  await (await db()).put('chunks', payload, cacheKey('c', bookId, position));
}

/** 清除单本书的内容缓存（详情/分块/文件/大小；进度另行保留） */
export async function clearBookContent(bookId: number): Promise<void> {
  const database = await db();
  const prefixes = [
    cacheKey('b', bookId),
    cacheKey('c', bookId) + ':',
    cacheKey('f', bookId),
  ];
  // 顺序执行：IDB 事务按序提交即可，无需并行
  // 顺序执行：IDB 事务按序提交即可，无需并行
  for (const store of ['meta', 'chunks', 'files'] as const) {
    // oxlint-disable-next-line no-await-in-loop
    const keys = await database.getAllKeys(store);
    for (const key of keys) {
      if (prefixes.some(prefix => key.startsWith(prefix))) {
        // oxlint-disable-next-line no-await-in-loop
        await database.delete(store, key);
      }
    }
  }
  await database.delete('kv', cacheKey('fmeta', bookId));
}

/** 已缓存整本文件的字节数；未缓存返回 null */
export async function fileBytes(bookId: number): Promise<null | number> {
  try {
    const value = (await (await db()).get('kv', cacheKey('fmeta', bookId))) as
      | number
      | undefined;
    return typeof value === 'number' ? value : null;
  } catch {
    return null;
  }
}

// --- 书籍详情 ---

export async function fileExists(bookId: number): Promise<boolean> {
  try {
    return (await (await db()).count('files', cacheKey('f', bookId))) > 0;
  } catch {
    return false;
  }
}

export async function fileGet(bookId: number): Promise<ArrayBuffer | null> {
  try {
    const cached = await (await db()).get('files', cacheKey('f', bookId));
    return cached?.data ?? null;
  } catch (err) {
    console.warn('offline file read failed', err);
    return null;
  }
}

// --- txt 分块 ---

export async function filePut(
  bookId: number,
  data: ArrayBuffer,
  rev: string,
): Promise<void> {
  await (await db()).put('files', { data, rev }, cacheKey('f', bookId));
  // 大小单独落 kv：面板展示无需整读 ArrayBuffer
  await (await db()).put('kv', data.byteLength, cacheKey('fmeta', bookId));
}

export async function kvGet<T>(key: string): Promise<null | T> {
  try {
    return (
      ((await (await db()).get('kv', cacheKey(key))) as T | undefined) ?? null
    );
  } catch (err) {
    console.warn('offline kv read failed', err);
    return null;
  }
}

export async function kvPut(key: string, value: unknown): Promise<boolean> {
  try {
    await (await db()).put('kv', value, cacheKey(key));
    return true;
  } catch (err) {
    console.warn('offline kv write failed', err);
    return false;
  }
}

// --- epub/pdf 整本文件 ---

export async function metaGet(bookId: number): Promise<CachedMeta | null> {
  try {
    return (await (await db()).get('meta', cacheKey('b', bookId))) ?? null;
  } catch (err) {
    console.warn('offline meta read failed', err);
    return null;
  }
}

/** quota 超限时 put 直接抛错，由调用方统一甄别（isQuotaError） */
export async function metaPut(
  bookId: number,
  value: CachedMeta,
): Promise<void> {
  await (await db()).put('meta', value, cacheKey('b', bookId));
}

/** 缓存是否可用（极端环境禁用 IDB 时静默降级为纯在线） */
export function offlineStorageSupported(): boolean {
  return typeof indexedDB !== 'undefined';
}

function db(): Promise<IDBPDatabase<OfflineDB>> {
  dbPromise ??= openDB<OfflineDB>(DB_NAME, DB_VERSION, {
    upgrade(database) {
      for (const name of ['kv', 'meta', 'chunks', 'files'] as const) {
        database.createObjectStore(name);
      }
    },
  });
  return dbPromise;
}

// --- 清理 ---

function parseJwtSub(token: null | string): null | string {
  if (!token) return null;
  const payload = token.split('.')[1];
  if (!payload) return null;
  try {
    const json = JSON.parse(
      atob(payload.replace(/-/g, '+').replace(/_/g, '/')),
    ) as {
      sub?: unknown;
    };
    return typeof json.sub === 'string' ? json.sub : null;
  } catch {
    return null;
  }
}

// 缓存按用户隔离；登出后缓存保留（token 仍在本地即可离线读取）
function userId(): string {
  // 不做记忆化：同页登出再换号登录时前缀必须随之切换
  return parseJwtSub(getToken('user')) ?? 'anon';
}
