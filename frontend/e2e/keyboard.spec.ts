// 键盘操作冒烟：space / enter / page down / 方向上下键翻页（滚动模式为
// 章内滚动），page up / shift+space 反向，左右方向键切章。按键映射的
// 纯逻辑单测在 reader/keyboard.test.ts，本用例覆盖阅读器视图的实际接线。
import { fileURLToPath } from 'node:url';

import {
  expect,
  test,
  type APIRequestContext,
  type Page,
} from '@playwright/test';

import { createUser, seedSession, updateSettings, uploadBook } from './helpers';

const API = '/api/v1';
const BOOK_PATH = fileURLToPath(new URL('./assets/novel.txt', import.meta.url));
// 覆盖章内翻页动画时长（280ms），保证读取的是落定位置
const SETTLE_MS = 400;

/** 书籍章节起始字符位置（按 position 升序） */
async function chapterPositions(
  request: APIRequestContext,
  token: string,
  bookId: number,
): Promise<number[]> {
  const res = await request.get(`${API}/books/${bookId}`, {
    headers: { authorization: `Bearer ${token}` },
  });
  expect(res.ok()).toBe(true);
  const { data } = (await res.json()) as {
    data: { chapters: { position: number }[] };
  };
  return data.chapters.map(chapter => chapter.position);
}

async function settledTop(page: Page): Promise<number> {
  await page.waitForTimeout(SETTLE_MS);
  const at = await topOffset(page);
  if (at === null) throw new Error('scroll container not ready');
  return at;
}

/** 滚动容器视口顶部段落的字符偏移（与 Reader 的定位口径一致） */
function topOffset(page: Page): Promise<null | number> {
  return page.evaluate(() => {
    const el = document.querySelector<HTMLElement>('.overflow-y-auto');
    if (!el) return null;
    const base = el.offsetTop;
    let best: null | number = null;
    for (const p of el.querySelectorAll<HTMLElement>('[data-pos]')) {
      if (p.offsetTop - base <= el.scrollTop + 12) best = Number(p.dataset.pos);
      else break;
    }
    return best;
  });
}

test('滚动模式：page down 滚动，左右方向键切章', async ({ page }) => {
  const user = await createUser(page.context().request, 'kbd');
  seedSession(page, user.token);
  const request = page.context().request;
  const book = await uploadBook(request, user.token, BOOK_PATH);
  const chapters = await chapterPositions(request, user.token, book.book_id);
  const ch2 = chapters.at(1);
  if (ch2 === undefined) throw new Error('expected multi-chapter book');

  // 服务端 schema 默认即滚动模式
  await page.goto(`/read/${book.book_id}`);
  await expect(page.locator('[data-pos]').first()).toBeVisible();
  await expect.poll(() => topOffset(page)).toBe(0);

  // page down / 方向下键：章内滚动 90% 视口
  await page.keyboard.press('PageDown');
  const afterPageDown = await settledTop(page);
  expect(afterPageDown).toBeGreaterThan(0);
  await page.keyboard.press('ArrowDown');
  expect(await settledTop(page)).toBeGreaterThan(afterPageDown);

  // 右方向键：切到下一章顶部（第二章标题段落的 data-pos 即章节 position）
  await page.keyboard.press('ArrowRight');
  await expect.poll(() => topOffset(page)).toBe(ch2);

  // space：新章内继续向下滚动
  await page.keyboard.press(' ');
  expect(await settledTop(page)).toBeGreaterThan(ch2);

  // 左方向键：回到上一章（落点为章末衔接处，仍处于第一章范围）
  await page.keyboard.press('ArrowLeft');
  await expect.poll(() => topOffset(page)).toBeLessThan(ch2);
});

test('分页模式：space/enter 翻页，shift+space 反向，右方向键切章', async ({
  page,
}) => {
  const user = await createUser(page.context().request, 'kbdpg');
  seedSession(page, user.token);
  const request = page.context().request;
  await updateSettings(request, user.token, { pagination_mode: 'paginate' });
  const book = await uploadBook(request, user.token, BOOK_PATH);
  const chapters = await chapterPositions(request, user.token, book.book_id);

  await page.goto(`/read/${book.book_id}`);
  const indicator = page.getByText(/^\d+ \/ \d+$/);
  await expect(indicator).toBeVisible();
  await expect(indicator).toHaveText(/^1 \/ \d+$/);

  await page.keyboard.press(' ');
  await expect(indicator).toHaveText(/^2 \/ \d+$/);
  await page.keyboard.press('Enter');
  await expect(indicator).toHaveText(/^3 \/ \d+$/);
  await page.keyboard.press('Shift+Space');
  await expect(indicator).toHaveText(/^2 \/ \d+$/);
  await page.keyboard.press('PageUp');
  await expect(indicator).toHaveText(/^1 \/ \d+$/);

  // 右方向键切章：跳到第二章起始页。第一章远长于一页，首页不含其段落，
  // 该段落在 DOM 中出现即代表切章生效
  const ch2 = chapters.at(1);
  if (ch2 === undefined) throw new Error('expected multi-chapter book');
  await page.keyboard.press('ArrowRight');
  await expect
    .poll(() =>
      page.evaluate(
        pos => Boolean(document.querySelector(`p[data-pos="${pos}"]`)),
        ch2,
      ),
    )
    .toBe(true);
});
