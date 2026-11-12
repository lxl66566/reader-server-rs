// epub.js OPF 目录模式（懒加载）：服务端按 zip 条目逐个下发，打开书
// 不再整本下载。两类请求路径：
// - OPF / NCX / 章节 xhtml：epub.js 经 book.load → requestMethod 主动拉取
//   （locations 生成也走这里，章节会逐个拉全）
// - css / img / font：iframe 内浏览器经 <base> 直连条目端点，不经本模块

import ePub, { type Book } from 'epubjs';
import type { BookOptions } from 'epubjs/types/book';

import { bookApi } from '../api';

/** DOMParser 按这些扩展名走 text/xml（与 epub.js 内置 request 一致） */
const XML_EXTS = new Set(['ncx', 'opf', 'xml']);

/** URL 最后一段的扩展名（小写）；token 段在路径更早处，不会被误取 */
export function extOf(url: string): string {
  const clean = (url.split(/[?#]/, 1)[0] ?? '').split('/');
  const m = /\.([^./]+)$/.exec(clean.at(-1) ?? '');
  return (m?.[1] ?? '').toLowerCase();
}

/** 429（限流）后的指数退避基数：locations 生成对大书密集拉取章节，
 * 串行请求仍可能触到服务端按 IP 的速率上限 */
const RATE_LIMIT_RETRY_BASE_MS = 500;
const RATE_LIMIT_RETRIES = 3;

type EpubRequestResult = Blob | Document | Record<string, unknown> | string;

/** epub.js 的 requestMethod 钩子：替换默认 XHR，走条目端点 */
export function makeEpubRequestMethod(): NonNullable<
  BookOptions['requestMethod']
> {
  // requestMethod 运行时也会返回纯文本（txt 等条目），epub.js 的类型声明
  // 收窄为 Promise<object> 与实现不符，在此边界处收窄
  return (async (url: string, type: null | string) =>
    fetchEpubEntry(url, (type ?? extOf(url)).toLowerCase())) as NonNullable<
    BookOptions['requestMethod']
  >;
}

/**
 * 以懒加载模式打开 epub：基址指向服务端条目端点（token 在路径段，末尾
 * 斜杠保证相对路径解析保留 token 段——Path.resolve 按目录语义解析）。
 * 章节按需拉取，css/img/font 经 <base> 相对解析自动携带鉴权
 */
export function openLazyEpub(bookId: number): Book {
  return ePub(bookApi.epubDirUrl(bookId), {
    openAs: 'directory',
    requestMethod: makeEpubRequestMethod(),
  });
}

async function fetchEpubEntry(
  url: string,
  type: string,
): Promise<EpubRequestResult> {
  let res = await fetch(url);
  for (let i = 0; res.status === 429 && i < RATE_LIMIT_RETRIES; i++) {
    // oxlint-disable-next-line no-await-in-loop
    await new Promise(resolve =>
      setTimeout(resolve, RATE_LIMIT_RETRY_BASE_MS * 2 ** i),
    );
    // oxlint-disable-next-line no-await-in-loop
    res = await fetch(url);
  }
  if (!res.ok) {
    throw new Error(`epub entry request failed: ${res.status} ${url}`);
  }
  if (type === 'blob') return res.blob();
  const text = await res.text();
  if (XML_EXTS.has(type)) {
    return new DOMParser().parseFromString(text, 'text/xml');
  }
  if (type === 'xhtml') {
    return new DOMParser().parseFromString(text, 'application/xhtml+xml');
  }
  if (type === 'html' || type === 'htm') {
    return new DOMParser().parseFromString(text, 'text/html');
  }
  if (type === 'json') return JSON.parse(text) as Record<string, unknown>;
  return text;
}
