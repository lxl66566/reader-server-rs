// EPUB 冒烟：样本书（e2e/assets/sample.epub，gen-assets.mjs 生成）验证
// 打开后进度上报心跳落库、两种翻页模式的 tap 分区、阅读主题覆盖书源样式。
import { fileURLToPath } from 'node:url';

import { expect, test, type Page } from '@playwright/test';

import {
  createUser,
  getBook,
  seedSession,
  updateSettings,
  uploadBook,
} from './helpers';

// fileURLToPath：pathname 在 Windows 上会得到 "/C:/..." 坏路径
const SAMPLE_EPUB = fileURLToPath(
  new URL('./assets/sample.epub', import.meta.url),
);

/** epub 章节 iframe 的文档句柄 */
function chapterFrame(page: Page) {
  return page.locator('iframe').first().contentFrame();
}

/** 正文列容器（epub 渲染目标）在视口中的矩形 */
async function columnRect(page: Page) {
  return page.evaluate(() => {
    const el = [...document.querySelectorAll('div')].find(
      d => d.className === 'mx-auto h-full w-full',
    );
    if (!el) throw new Error('epub column container not found');
    return el.getBoundingClientRect();
  });
}

/** epub 章节 iframe 在宿主视口中的 x 偏移（分页模式被平移时为负） */
async function iframeX(page: Page): Promise<number> {
  return page
    .locator('iframe')
    .first()
    .evaluate(el => Math.round(el.getBoundingClientRect().x));
}

async function scrollerTop(page: Page): Promise<number> {
  return page.evaluate(() => {
    const el = document.querySelector('.epub-container');
    return el ? Math.round(el.scrollTop) : -1;
  });
}

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

test('分页模式翻页后 tap 分区不偏移', async ({ page }) => {
  // 回归：epub.js 分页翻页靠平移 iframe 实现，iframe 内事件坐标处于
  // iframe 坐标系；分区若按宿主坐标判定，翻过一页后所有 tap 都落入
  // “右区”——左区回退与中区唤出菜单全部失效
  const user = await createUser(page.context().request, 'epubpg');
  seedSession(page, user.token);
  const request = page.context().request;
  await updateSettings(request, user.token, { pagination_mode: 'paginate' });
  const book = await uploadBook(request, user.token, SAMPLE_EPUB);

  await page.goto(`/read/${book.book_id}`);
  await expect(page.locator('iframe').first()).toBeVisible();
  // 章内翻到第 2 页（iframe 被平移，x < 0）；样章超过一屏，最多翻 3 次
  await expect
    .poll(
      async () => {
        const x = await iframeX(page);
        if (x >= 0) await page.keyboard.press('ArrowRight');
        return x;
      },
      { intervals: [500, 800], timeout: 15_000 },
    )
    .toBeLessThan(0);
  const shifted = await iframeX(page);

  const rect = await columnRect(page);
  // 左区 tap：应回退一页（x 向 0 方向增大），而不是继续前进
  await page.mouse.click(
    rect.left + rect.width * 0.15,
    rect.top + rect.height / 2,
  );
  await expect
    .poll(() => iframeX(page), { intervals: [300, 600], timeout: 8000 })
    .toBeGreaterThan(shifted);

  // 中区 tap：唤出菜单且页面不动
  const beforeCenter = await iframeX(page);
  await page.mouse.click(
    rect.left + rect.width / 2,
    rect.top + rect.height / 2,
  );
  await expect(page.getByRole('button', { name: '目录' })).toBeVisible();
  expect(await iframeX(page)).toBe(beforeCenter);
  // 点遮罩关闭
  await page.mouse.click(viewportSize(page).vw - 5, viewportSize(page).vh / 2);
  await expect(page.getByRole('button', { name: '目录' })).toBeHidden();
});

test('滚动模式 tap 上/下分区滚动、贴边切章、中区唤出菜单', async ({ page }) => {
  const user = await createUser(page.context().request, 'eptap');
  seedSession(page, user.token);
  const request = page.context().request;
  await updateSettings(request, user.token, { pagination_mode: 'scroll' });
  const book = await uploadBook(request, user.token, SAMPLE_EPUB);

  await page.goto(`/read/${book.book_id}`);
  await expect(page.locator('iframe').first()).toBeVisible();
  const { vh, vw } = viewportSize(page);

  // 中区：唤出菜单
  await page.mouse.click(vw / 2, vh / 2);
  await expect(page.getByRole('button', { name: '目录' })).toBeVisible();
  await page.mouse.click(vw - 5, vh / 2); // 点遮罩关闭
  await expect(page.getByRole('button', { name: '目录' })).toBeHidden();

  // 下区：章内下滚 90% 视口（样章超过一屏），不唤出菜单
  await page.mouse.click(vw / 2, vh * 0.9);
  await expect(page.getByRole('button', { name: '目录' })).toBeHidden();
  await expect.poll(() => scrollerTop(page)).toBeGreaterThan(0);

  // 点下区直至贴边，再点一次 → 切下一章（而不是停在章尾）
  await expect
    .poll(
      async () => {
        const atBottom = await page.evaluate(() => {
          const el = document.querySelector('.epub-container');
          return !!el && el.scrollTop >= el.scrollHeight - el.clientHeight - 2;
        });
        if (!atBottom) await page.mouse.click(vw / 2, vh * 0.9);
        return atBottom;
      },
      { intervals: [400, 700], timeout: 10_000 },
    )
    .toBe(true);
  await page.mouse.click(vw / 2, vh * 0.9);
  await expect
    .poll(() => chapterFrame(page).locator('h1').textContent(), {
      intervals: [500, 800],
      timeout: 10_000,
    })
    .toContain('第2章');

  // 上区：顶部贴边时切回上一章
  await page.mouse.click(vw / 2, vh * 0.05);
  await expect
    .poll(() => chapterFrame(page).locator('h1').textContent(), {
      intervals: [500, 800],
      timeout: 10_000,
    })
    .toContain('第1章');
});

test('阅读主题覆盖书源强制黑字白底', async ({ page }) => {
  // 回归：书源 CSS 常以 !important 直接锁定元素颜色（p{color:#000}），
  // 主题只在 body 上设色时会被直接规则击败，暗色背景下正文不可读
  const user = await createUser(page.context().request, 'epubcss');
  seedSession(page, user.token);
  const request = page.context().request;
  await updateSettings(request, user.token, {
    background_color: '#101010',
    pagination_mode: 'scroll',
    text_color: '#abcdef',
  });
  const book = await uploadBook(request, user.token, SAMPLE_EPUB);

  await page.goto(`/read/${book.book_id}`);
  const frame = chapterFrame(page);
  await expect(frame.locator('p').first()).toBeVisible();
  // 正文段落颜色跟随用户设置，而不是书源的 #000
  await expect
    .poll(() =>
      frame
        .locator('p')
        .first()
        .evaluate(el => getComputedStyle(el).color),
    )
    .toBe('rgb(171, 205, 239)');
  // 书源背景（h1 灰底）被压为透明，阅读背景由宿主呈现
  await expect
    .poll(() =>
      frame
        .locator('h1')
        .first()
        .evaluate(el => getComputedStyle(el).backgroundColor),
    )
    .toBe('rgba(0, 0, 0, 0)');
});
