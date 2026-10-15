// 离线阅读冒烟：在线缓存全本（txt / epub）→ 等待 SW 就绪 → 断网重载
// → 页面外壳与内容均来自本地缓存，且可继续阅读。
// 目标必须是构建产物实例（默认 :3000）；vite dev server 无 SW。
import { expect, test, type Page } from '@playwright/test';

import { createUser, seedSession, uploadBook } from './helpers';

const NOVEL_TXT = new URL('./assets/novel.txt', import.meta.url).pathname;
const SAMPLE_EPUB = new URL('./assets/sample.epub', import.meta.url).pathname;

/** 打开离线缓存面板并缓存全本，等待状态回到已缓存 */
async function cacheWholeBook(
  page: Page,
  open: (p: Page) => Promise<void>,
): Promise<void> {
  // 唤出可能被增量加载的 Suspense 遮罩吞掉：点击后探测，未开则重试
  const offlineBtn = page.getByRole('button', { name: '离线缓存' });
  // oxlint-disable-next-line no-await-in-loop
  for (let i = 0; i < 5 && !(await offlineBtn.isVisible()); i++) {
    // oxlint-disable-next-line no-await-in-loop
    await open(page);
    // oxlint-disable-next-line no-await-in-loop
    await page.waitForTimeout(400);
  }
  await offlineBtn.click();
  await page.getByRole('button', { name: '缓存全本' }).click();
  // 下载完成后 toast 提示 + 面板按钮恢复可点
  await expect(page.getByText('缓存完成')).toBeVisible({ timeout: 30_000 });
}

/** 唤出阅读菜单：epub 手势只认 touch 事件，鼠标点击落在 iframe 下方的
 * 宿主区域（滚动模式短章必然留白）触发 onHostTap */
async function openChromeEpub(page: Page): Promise<void> {
  const { vh, vw } = viewportSize(page);
  await page.mouse.click(vw / 2, vh * 0.9);
}

/** 唤出阅读菜单：txt 点击屏幕正中（tap 中央区） */
async function openChromeTxt(page: Page): Promise<void> {
  const { vh, vw } = viewportSize(page);
  await page.mouse.click(vw / 2, vh / 2);
}

/** 视口尺寸（headless 下必有值，兜底只为类型收窄） */
function viewportSize(page: Page): { vh: number; vw: number } {
  const size = page.viewportSize();
  return { vh: size?.height ?? 800, vw: size?.width ?? 1280 };
}

/** 等待 Service Worker 激活并接管当前页面 */
async function waitForServiceWorker(page: Page): Promise<void> {
  await page.evaluate(async () => {
    await navigator.serviceWorker.ready;
  });
  // 重新加载后新的页面才会被 SW 控制
  await page.reload();
  await expect
    .poll(() =>
      page.evaluate(() =>
        navigator.serviceWorker.controller !== null ? 'controlled' : 'none',
      ),
    )
    .toBe('controlled');
}

test('txt 缓存全本后断网可继续阅读', async ({ page }) => {
  const user = await createUser(page.context().request, 'offtxt');
  seedSession(page, user.token);
  const request = page.context().request;
  const book = await uploadBook(request, user.token, NOVEL_TXT);

  await page.goto(`/read/${book.book_id}`);
  await expect(page.locator('[data-pos]').first()).toBeVisible();
  await cacheWholeBook(page, openChromeTxt);

  await waitForServiceWorker(page);
  await page.context().setOffline(true);

  // 断网重载：外壳（SW 预缓存）+ 详情/内容（IDB）全部本地
  await page.reload();
  await expect(page.locator('[data-pos]').first()).toBeVisible({
    timeout: 20_000,
  });
  await expect(page.getByText('离线模式')).toBeVisible();

  // 离线滚动阅读：视口顶部段落位置前进
  const offsetBefore = await topOffset(page);
  const { vw } = viewportSize(page);
  await page.mouse.move(vw / 2, 400);
  for (let i = 0; i < 8; i++) {
    // oxlint-disable-next-line no-await-in-loop
    await page.mouse.wheel(0, 600);
    // oxlint-disable-next-line no-await-in-loop
    await page.waitForTimeout(120);
  }
  const offsetAfter = await topOffset(page);
  expect(offsetAfter).not.toBeNull();
  expect(offsetAfter).toBeGreaterThan(offsetBefore ?? 0);
});

test('epub 缓存全本后断网可打开', async ({ page }) => {
  const user = await createUser(page.context().request, 'offepub');
  seedSession(page, user.token);
  const request = page.context().request;
  const book = await uploadBook(request, user.token, SAMPLE_EPUB);

  await page.goto(`/read/${book.book_id}`);
  await expect(page.locator('iframe').first()).toBeVisible();
  await cacheWholeBook(page, openChromeEpub);

  await waitForServiceWorker(page);
  await page.context().setOffline(true);

  await page.reload();
  await expect(page.locator('iframe').first()).toBeVisible({
    timeout: 20_000,
  });
  await expect(page.getByText('离线模式')).toBeVisible();
});

/** 滚动容器视口顶部段落的字符偏移（与 Reader 的定位口径一致） */
function topOffset(page: Page): Promise<null | number> {
  return page.evaluate(() => {
    const para = document.querySelector<HTMLElement>('[data-pos]');
    const el = para?.closest<HTMLElement>('.overflow-y-auto');
    if (!el || !para) return null;
    const base = el.offsetTop;
    let best: null | number = null;
    for (const p of el.querySelectorAll<HTMLElement>('[data-pos]')) {
      if (p.offsetTop - base <= el.scrollTop + 8) best = Number(p.dataset.pos);
      else break;
    }
    return best;
  });
}
