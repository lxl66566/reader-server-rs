// TXT 阅读冒烟：UI 上传 → 详情页进入阅读 → 分页翻页 → 切滚动模式 →
// 滚动产生进度 → 刷新后进度保留（心跳落库 + 位置恢复）。
import { writeFileSync } from 'node:fs';

import { expect, test, type Page } from '@playwright/test';

import {
  createUser,
  getBook,
  getSettings,
  seedSession,
  updateSettings,
  uploadBook,
} from './helpers';

// 测试书入库于 e2e/assets/（gen-assets.mjs 生成）；超长单章样本体积较大，
// 由下方用例运行时生成
const BOOK_PATH = new URL('./assets/novel.txt', import.meta.url).pathname;
const LOCATOR_TOLERANCE = 5000;

/** 点击右缘遮罩层关闭 chrome（遮罩挡住内容区的滚轮/点击） */
async function closeChrome(page: Page): Promise<void> {
  const { vh, vw } = viewportSize(page);
  await page.mouse.click(vw - 5, vh / 2);
}

/** 唤出阅读菜单：屏幕正中属于 tap 中央区（居中容器下同样成立） */
async function openChrome(page: Page): Promise<void> {
  const { vh, vw } = viewportSize(page);
  await page.mouse.click(vw / 2, vh / 2);
}

/** 滚动容器视口顶部段落的字符偏移（与 Reader 的定位口径一致） */
function topOffset(page: Page): Promise<null | number> {
  return page.evaluate(() => {
    const el = document.querySelector<HTMLElement>('.overflow-y-auto');
    if (!el) return null;
    const base = el.offsetTop;
    let best: null | number = null;
    for (const p of el.querySelectorAll<HTMLElement>('[data-pos]')) {
      if (p.offsetTop - base <= el.scrollTop + 8) best = Number(p.dataset.pos);
      else break;
    }
    return best;
  });
}

/** 视口尺寸（headless 下必有值，兜底只为类型收窄） */
function viewportSize(page: Page): { vh: number; vw: number } {
  const size = page.viewportSize();
  return { vh: size?.height ?? 800, vw: size?.width ?? 1280 };
}

test('上传 txt 并保留阅读进度', async ({ page }) => {
  const title = `e2e-txt-${Math.random().toString(36).slice(2, 8)}`;
  const user = await createUser(page.context().request, 'read');
  seedSession(page, user.token);
  const request = page.context().request;

  // --- UI 上传 ---
  await page.goto('/');
  await page.locator('main button', { hasText: '上传书籍' }).click();
  await page.locator('input[type="file"]').setInputFiles(BOOK_PATH);
  const modal = page.locator('div.fixed.inset-0');
  await modal.getByLabel('书名').fill(title);
  await modal.getByRole('button', { exact: true, name: '上传' }).click();
  await expect(page.getByText(title)).toBeVisible();

  // 固定初始模式为分页（服务端 schema 默认是滚动，避免环境差异）。
  // settings store 在同一次页面会话内不会重新拉取：刷新让 API 级修改生效
  await updateSettings(request, user.token, { pagination_mode: 'paginate' });
  await page.reload();
  await expect(page.getByText(title)).toBeVisible();

  // --- 详情页 → 阅读页（分页模式） ---
  // 卡片整面被 absolute 的“阅读”按钮覆盖（直接进阅读页），进详情页走角标按钮
  await page.getByRole('button', { name: '查看详情' }).first().click();
  await expect(page).toHaveURL(/\/book\/\d+/);
  await page.getByRole('button', { name: '开始阅读' }).click();
  await expect(page).toHaveURL(/\/read\/\d+/);
  // 分页指示器（readerPageOf = "{page} / {total}"）出现说明测量与渲染完成
  const indicator = page.getByText(/^\d+ \/ \d+$/);
  await expect(indicator).toBeVisible();
  await expect(indicator).toHaveText(/^1 \/ \d+$/);

  // 键盘翻页
  await page.keyboard.press('ArrowRight');
  await expect(indicator).toHaveText(/^2 \/ \d+$/);

  // --- 唤出 chrome，切到滚动模式 ---
  await openChrome(page);
  await expect(page.getByRole('button', { name: '目录' })).toBeVisible();
  await page.getByRole('button', { exact: true, name: '设置' }).click();
  const modeRow = page.locator('div.py-2', { hasText: '翻页模式' });
  await modeRow.locator('select').selectOption({ label: '上下滚动' });
  await page.keyboard.press('Escape');
  // chrome 打开时遮罩层挡住滚轮，点击遮罩关闭它
  await closeChrome(page);
  await expect(page.getByRole('button', { name: '目录' })).toBeHidden();

  // 模式切换是 500ms 防抖 PUT：等落库后再 reload，否则挂起的补丁会丢
  await expect
    .poll(
      async () => (await getSettings(request, user.token)).pagination_mode,
      { intervals: [500], timeout: 10_000 },
    )
    .toBe('scroll');

  // --- 滚动阅读，等待心跳把进度写到服务端 ---
  const scroller = page.locator('.overflow-y-auto');
  await expect(scroller).toBeVisible();
  const { vh, vw } = viewportSize(page);
  await page.mouse.move(vw / 2, vh / 2);
  await page.mouse.wheel(0, 2500);
  await page.mouse.wheel(0, 2500);
  await page.mouse.wheel(0, 2500);
  const uiOffset = await topOffset(page);
  expect(uiOffset).not.toBeNull();
  await expect
    .poll(
      async () => {
        const book = await getBook(
          request,
          user.token,
          bookIdFromUrl(page.url()),
        );
        return book.locator === null
          ? -LOCATOR_TOLERANCE * 2
          : Math.abs(Number(book.locator) - (uiOffset ?? 0));
      },
      { intervals: [1000], timeout: 20_000 },
    )
    .toBeLessThan(LOCATOR_TOLERANCE);

  // --- 刷新后进度恢复 ---
  await page.reload();
  await expect(scroller).toBeVisible();
  await expect
    .poll(() => topOffset(page), { intervals: [500], timeout: 15_000 })
    .not.toBeNull();
  const restored = await topOffset(page);
  const book = await getBook(request, user.token, bookIdFromUrl(page.url()));
  expect(restored).not.toBeNull();
  expect(Math.abs((restored ?? 0) - Number(book.locator ?? 0))).toBeLessThan(
    LOCATOR_TOLERANCE,
  );
  expect(book.percent).toBeGreaterThan(0);
});

/** 滚动容器的 scrollTop */
function scrollTopOf(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      document.querySelector<HTMLElement>('.overflow-y-auto')?.scrollTop ?? 0,
  );
}

test('移动端滚动模式触摸 tap 可用', async ({ page }) => {
  // 触摸模拟只在触屏项目（mobile）可用
  test.skip(test.info().project.use.hasTouch !== true, '需要触屏项目');
  // 回归：ScrollView 曾用 onPointerUp 判 tap，可滚动容器上真实触摸的
  // pointerup 被 pointercancel 取代，触摸 tap 完全失效
  const user = await createUser(page.context().request, 'touch');
  seedSession(page, user.token);
  const request = page.context().request;
  const book = await uploadBook(request, user.token, BOOK_PATH);

  // 深链定位到第一章中段（章前约 8000 字、章后仍余约 4000 字）：
  // tap 底部在章内滚动，而非贴边触发切章
  await page.goto(`/read/${book.book_id}?locator=8000`);
  const scroller = page.locator('.overflow-y-auto');
  await expect(scroller).toBeVisible();
  await expect(scroller.locator('p').first()).toBeVisible();

  const { vh, vw } = viewportSize(page);
  // 触摸 tap 正中：唤出菜单
  await page.touchscreen.tap(vw / 2, vh / 2);
  await expect(page.getByRole('button', { name: '目录' })).toBeVisible();
  await closeChrome(page);
  await expect(page.getByRole('button', { name: '目录' })).toBeHidden();

  const before = await scrollTopOf(page);
  expect(before).toBeGreaterThan(0);
  // 触摸 tap 下 1/3：向前滚动约一屏（等待 280ms 翻页动画结束再断言）
  await page.touchscreen.tap(vw / 2, vh * 0.85);
  await page.waitForTimeout(500);
  const down = await scrollTopOf(page);
  expect(down).toBeGreaterThan(before);
  // 触摸 tap 上 1/3：向后滚动
  await page.touchscreen.tap(vw / 2, vh * 0.1);
  await page.waitForTimeout(500);
  expect(await scrollTopOf(page)).toBeLessThan(down);
});

/** 从当前 URL 提取书籍 id */
function bookIdFromUrl(url: string): number {
  const match = /\/read\/(\d+)/.exec(url);
  if (!match?.[1]) throw new Error(`no book id in url: ${url}`);
  return Number(match[1]);
}

test('超长单章增量加载期间视口位置不回退', async ({ page }) => {
  // 回归：章节切分失败的超长单章在滚动模式增量加载时，曾因“先换窗后
  // 取数 + 取数完成后按旧锚点绝对回写 scrollTop”，取数期间的用户滚动
  // 被整体回滚，视口内容每次加载跳回约五分之一屏
  const user = await createUser(page.context().request, 'onechap');
  seedSession(page, user.token);
  const request = page.context().request;
  await updateSettings(request, user.token, { pagination_mode: 'scroll' });

  // 无标题行且每行远超 30 字（不命中章节规则 → 切分失败成单章）；
  // 体积需明显超过滚动窗口的 9 chunk 上限才会触发边加载边回收
  const paragraphs: string[] = [];
  // 内容均为 BMP 字符，码元数即码点数，与服务端偏移口径一致
  for (let written = 0; written < 140_000;) {
    const line = `这是没有任何章节标题结构的普通正文段落，编号${paragraphs.length + 1}，填充文字用于撑起超过滚动窗口上限的超长单章。`;
    paragraphs.push(line);
    written += line.length + 1;
  }
  const bookPath = '/tmp/reader-e2e-one-chapter.txt';
  writeFileSync(bookPath, paragraphs.join('\n'), 'utf8');
  const book = await uploadBook(request, user.token, bookPath);

  // 深链到中段：向下滚动先后经历窗口增长与到上限后的“边加载边回收”
  await page.goto(`/read/${book.book_id}?locator=77000`);
  const scroller = page.locator('.overflow-y-auto');
  await expect(scroller).toBeVisible();
  await expect.poll(() => topOffset(page), { timeout: 15_000 }).not.toBeNull();

  // 内容接口加延迟：稳定制造“取数期间用户仍在滚动”的时序
  await page.route(/\/api\/v1\/books\/\d+\/content/, async route => {
    await new Promise(resolve => setTimeout(resolve, 300));
    await route.continue();
  });

  const { vh, vw } = viewportSize(page);
  await page.mouse.move(vw / 2, vh / 2);
  const samples: number[] = [];
  for (let i = 0; i < 32; i++) {
    await page.mouse.wheel(0, 500);
    await page.waitForTimeout(90);
    const at = await topOffset(page);
    if (at !== null) samples.push(at);
  }
  // 在途请求与窗口提交全部落定后再终测一次
  await page.waitForTimeout(1200);
  const settled = await topOffset(page);
  expect(settled).not.toBeNull();

  expect(samples.length).toBeGreaterThan(1);
  let previous: number | undefined;
  for (const current of samples) {
    if (previous !== undefined) {
      // 滚动补偿不得让已读位置回退（样本序列必须单调不减）
      expect(
        current,
        `viewport offset regressed: ${samples.join(', ')}`,
      ).toBeGreaterThanOrEqual(previous);
    }
    previous = current;
  }
  expect(settled ?? 0).toBeGreaterThanOrEqual(samples.at(-1) ?? 0);
  expect(settled ?? 0).toBeGreaterThan(samples.at(0) ?? 0);
});

test('分页模式 tap 按容器分区而非屏幕分区', async ({ page }) => {
  // 回归：tap 分区曾按 clientX/容器宽 计算，PC 居中列布局下未减容器
  // 左偏移，屏幕正中被误判为右区翻页而非唤出菜单
  const user = await createUser(page.context().request, 'tapzone');
  seedSession(page, user.token);
  const request = page.context().request;
  await updateSettings(request, user.token, { pagination_mode: 'paginate' });
  const book = await uploadBook(request, user.token, BOOK_PATH);

  await page.goto(`/read/${book.book_id}`);
  const indicator = page.getByText(/^\d+ \/ \d+$/);
  await expect(indicator).toBeVisible();
  await expect(indicator).toHaveText(/^1 \/ \d+$/);

  const { vh, vw } = viewportSize(page);
  // 屏幕正中（居中容器的中央区）：唤出菜单而非翻页
  await page.mouse.click(vw / 2, vh / 2);
  await expect(page.getByRole('button', { name: '目录' })).toBeVisible();
  await closeChrome(page);

  // 容器右 1/3：翻下一页
  const container = await page.evaluate(() => {
    const el = document.querySelector<HTMLElement>('div.overflow-hidden');
    if (!el) throw new Error('paginate container not found');
    const rect = el.getBoundingClientRect();
    return { left: rect.left, width: rect.width };
  });
  await page.mouse.click(container.left + container.width * 0.85, vh / 2);
  await expect(indicator).toHaveText(/^2 \/ \d+$/);
  // 容器左 1/3：翻回上一页
  await page.mouse.click(container.left + container.width * 0.15, vh / 2);
  await expect(indicator).toHaveText(/^1 \/ \d+$/);
});
