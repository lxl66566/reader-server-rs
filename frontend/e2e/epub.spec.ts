// EPUB 冒烟：样本书（e2e/assets/sample.epub，gen-assets.mjs 生成）验证
// 打开后进度上报心跳落库 + 滚动模式的 tap 行为。
import { expect, test, type Page } from '@playwright/test';

import {
  createUser,
  getBook,
  seedSession,
  updateSettings,
  uploadBook,
} from './helpers';

const SAMPLE_EPUB = new URL('./assets/sample.epub', import.meta.url).pathname;

/** 视口尺寸（headless 下必有值，兜底只为类型收窄） */
function viewportSize(page: Page): { vh: number; vw: number } {
  const size = page.viewportSize();
  return { vh: size?.height ?? 800, vw: size?.width ?? 1280 };
}

test('epub 打开后进度上报并落库', async ({ page }) => {
  // 回归：locations 未就绪时 onPosition 曾直接 return，零心跳、进度完全不
  // 保存。样本书较小、locations 很快就绪，本用例覆盖“翻页即上报”主路径
  const user = await createUser(page.context().request, 'epub');
  seedSession(page, user.token);
  const request = page.context().request;
  await updateSettings(request, user.token, { pagination_mode: 'paginate' });
  const book = await uploadBook(request, user.token, SAMPLE_EPUB);

  await page.goto(`/read/${book.book_id}`);
  await expect(page.locator('iframe').first()).toBeVisible();
  // rendition 就绪前按键无效：边翻页边轮询，直到心跳把进度写到服务端
  await expect
    .poll(
      async () => {
        await page.keyboard.press('ArrowRight');
        return (await getBook(request, user.token, book.book_id)).percent;
      },
      { intervals: [1000, 2000], timeout: 25_000 },
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
  const book = await uploadBook(request, user.token, SAMPLE_EPUB);

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
