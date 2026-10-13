// EPUB 回归：依赖项目根目录的 1.epub（未入库的真实大书，587 个内部文件），
// 缺失则跳过。覆盖大书心跳落库与滚动模式的 tap 行为。
import { existsSync } from 'node:fs';

import { expect, test, type Page } from '@playwright/test';

import {
  createUser,
  getBook,
  seedSession,
  updateSettings,
  uploadBook,
} from './helpers';

const BIG_EPUB = new URL('../../1.epub', import.meta.url).pathname;

test.skip(!existsSync(BIG_EPUB), '需要项目根目录的 1.epub（未入库的真实大书）');

// 上传大书 + locations 15s 超时降级 + 心跳落库，整体超出默认 60s
test.setTimeout(120_000);

/** 视口尺寸（headless 下必有值，兜底只为类型收窄） */
function viewportSize(page: Page): { vh: number; vw: number } {
  const size = page.viewportSize();
  return { vh: size?.height ?? 800, vw: size?.width ?? 1280 };
}

test('大 epub 打开后心跳尽快落库', async ({ page }) => {
  const user = await createUser(page.context().request, 'epub');
  seedSession(page, user.token);
  const request = page.context().request;
  const book = await uploadBook(request, user.token, BIG_EPUB);

  await page.goto(`/read/${book.book_id}`);
  // 回归：locations 未就绪时 onPosition 曾直接 return，零心跳、
  // 进度完全不保存。现在 15s 超时后降级为 spine 估算恢复上报
  await expect
    .poll(
      async () => (await getBook(request, user.token, book.book_id)).percent,
      {
        intervals: [1000, 2000],
        timeout: 25_000,
      },
    )
    .toBeGreaterThan(0);
});

test('滚动模式 tap 任意处唤出菜单', async ({ page }) => {
  // 回归：滚动模式 tap 曾按 manager 容器 scrollTop/clientHeight 换算
  // 分区，iframe 实际高 ≠ 容器高时错位（点下半屏翻页而非唤出菜单），
  // iframe 外的宿主区域则完全无响应。现简化为任意 tap 唤出菜单。
  const user = await createUser(page.context().request, 'eptap');
  seedSession(page, user.token);
  const request = page.context().request;
  await updateSettings(request, user.token, { pagination_mode: 'scroll' });
  const book = await uploadBook(request, user.token, BIG_EPUB);

  await page.goto(`/read/${book.book_id}`);
  await expect(page.locator('iframe').first()).toBeVisible();

  const { vh, vw } = viewportSize(page);
  // 下 1/3（无论落在 iframe 内还是宿主区域）：应唤出菜单而非翻页
  await page.mouse.click(vw / 2, vh * 0.9);
  await expect(page.getByRole('button', { name: '目录' })).toBeVisible();
  await page.mouse.click(vw - 5, vh / 2); // 点遮罩关闭
  await expect(page.getByRole('button', { name: '目录' })).toBeHidden();

  // 上 1/3：同样唤出菜单
  await page.mouse.click(vw / 2, vh * 0.05);
  await expect(page.getByRole('button', { name: '目录' })).toBeVisible();
});
