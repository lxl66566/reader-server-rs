import { readFileSync, writeFileSync } from 'node:fs';
// TXT 阅读冒烟：UI 上传 → 详情页进入阅读 → 分页翻页 → 切滚动模式 →
// 滚动产生进度 → 刷新后进度保留（心跳落库 + 位置恢复）。
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { expect, test, type Page } from '@playwright/test';

import {
  createUser,
  getBook,
  getSettings,
  seedReadingProgress,
  seedSession,
  updateSettings,
  uploadBook,
} from './helpers';

// 测试书入库于 e2e/assets/（gen-assets.mjs 生成）；超长单章样本体积较大，
// 由下方用例运行时生成
const BOOK_PATH = fileURLToPath(new URL('./assets/novel.txt', import.meta.url));
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
    // 容差与 TxtReader 的 JUMP_TOP_INSET_PX 对齐：跳转落点（段落顶距
    // 视口顶 12px 留白）必须读回该段落本身，否则永远差一段
    for (const p of el.querySelectorAll<HTMLElement>('[data-pos]')) {
      if (p.offsetTop - base <= el.scrollTop + 12) best = Number(p.dataset.pos);
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

  // 深链跳转是异步的（加载窗口 + rAF 定位）：等其生效再取样，
  // 否则可能先于跳转完成读到 scrollTop=0（构建产物加载快时更易触发）
  await expect.poll(() => scrollTopOf(page)).toBeGreaterThan(0);
  const before = await scrollTopOf(page);
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

test('滚动模式关闭重开标签页后恢复位置不回退', async ({ page: page1 }) => {
  // 回归：恢复定位曾按 content-visibility 塌缩布局的 offsetTop 直接写
  // scrollTop，重开页面后位置回退一到数页，且回退后的位置会被心跳写回
  // 服务端，进度永久丢失
  const request = page1.context().request;
  const user = await createUser(request, 'restore');
  seedSession(page1, user.token);
  await updateSettings(request, user.token, { pagination_mode: 'scroll' });
  const book = await uploadBook(request, user.token, BOOK_PATH);

  // --- 第一段阅读：滚几屏建立设备心跳与本地进度缓存，随后关闭标签页 ---
  await page1.goto(`/read/${book.book_id}`);
  const scroller = page1.locator('.overflow-y-auto');
  await expect(scroller).toBeVisible();
  await expect.poll(() => topOffset(page1), { timeout: 15_000 }).not.toBeNull();
  const { vh, vw } = viewportSize(page1);
  await page1.mouse.move(vw / 2, vh / 2);
  for (let i = 0; i < 4; i++) {
    // oxlint-disable-next-line no-await-in-loop
    await page1.mouse.wheel(0, Math.round(vh * 0.75));
    // oxlint-disable-next-line no-await-in-loop
    await page1.waitForTimeout(150);
  }
  await page1.waitForTimeout(2500); // 心跳落库 + IDB 写盘
  await page1.close();

  // --- 服务端进度播种到 chunk 深处（进度只能经 ws 心跳写入） ---
  // 11985 是 novel.txt 第一章内的段落起点（全书 31452 字），距 chunk 顶
  // （6000）约 90 段：塌缩布局下约 2500px，远超浏览器对 content-visibility
  // 的近屏展开余量，恢复时目标必然处于塌缩状态——正是回归场景
  const target = 11985;
  await seedReadingProgress(
    user.token,
    book.book_id,
    target,
    target / readFileSync(BOOK_PATH, 'utf8').length,
  );

  // --- 关闭后重开（同 context：localStorage 保留登录态，sessionStorage
  // 重建使设备标识变化，走服务端设备让位逻辑，等同真实关页重开） ---
  const page2 = await page1.context().newPage();
  seedSession(page2, user.token);
  await page2.goto(`/read/${book.book_id}`);
  await expect
    .poll(() => topOffset(page2), { intervals: [300], timeout: 15_000 })
    .toBe(target);

  // 重开后的心跳不得把更早的位置写回服务端（回归里进度曾被写回旧值）
  await page2.waitForTimeout(5000);
  const saved = await getBook(request, user.token, book.book_id);
  expect(saved.locator).toBe(String(target));
});

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
  // 系统临时目录：硬编码 /tmp 在 Windows 上不存在
  const bookPath = join(tmpdir(), 'reader-e2e-one-chapter.txt');
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
    // 轮询采样必须逐步 await，滚动窗口才会逐块加载
    // oxlint-disable-next-line no-await-in-loop
    await page.mouse.wheel(0, 500);
    // oxlint-disable-next-line no-await-in-loop
    await page.waitForTimeout(90);
    // oxlint-disable-next-line no-await-in-loop
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

/** 窗口内已渲染段落的最小字符偏移（窗口向上扩展的观测量） */
function minRenderedPos(page: Page): Promise<null | number> {
  return page.evaluate(() => {
    const els = [...document.querySelectorAll<HTMLElement>('[data-pos]')];
    return els.length
      ? Math.min(...els.map(el => Number(el.dataset.pos)))
      : null;
  });
}

test('返回上一章落在不足一屏的章末窗口时仍可向上加载', async ({ page }) => {
  // 回归：滚动模式切章是硬切换，返回上一章落在章末。当上一章末尾落在
  // chunk 边界后不久（或自定义正则切出的章尾本来就短）时，窗口内属于
  // 本章的段落不足一屏：容器不可滚动 → 永远不产生 scroll 事件 →
  // onScroll 里 scrollTop < 600 的 loadPrev 阈值永不触发，页面只剩
  // 章末几个段落且无法向上滚动。
  const user = await createUser(page.context().request, 'chapback');
  seedSession(page, user.token);
  const request = page.context().request;
  await updateSettings(request, user.token, { pagination_mode: 'scroll' });

  // 第一章 42004 字（跨 chunk 0..7），末行“尾声行”恰好起于 42000（chunk
  // 7 首字符）：返回上一章时窗口只含 chunk 7 中属于本章的这一行
  const lines: string[] = [];
  let at = 0;
  const push = (line: string): void => {
    lines.push(line);
    at += line.length + 1;
  };
  push('Part 1 开端');
  let n = 1;
  while (at + 50 <= 42000) {
    push(
      `第一章正文第${n}段，填充内容用于让章节跨越多个 chunk 边界。`.padEnd(
        49,
        '字',
      ),
    );
    n++;
  }
  push('尾'.repeat(42000 - at - 1)); // 对齐：下一行起点恰为 42000
  push('尾声行');
  push('Part 2 转折'); // offset 42004：第二章起点
  n = 1;
  while (at < 68000) {
    push(
      `第二章正文第${n}段，填充内容用于让深链定位落在第二章中段。`.padEnd(
        49,
        '字',
      ),
    );
    n++;
  }
  push('Part 3 结尾');
  push('第三章正文。');

  const bookPath = join(tmpdir(), 'reader-e2e-chapter-back.txt');
  writeFileSync(bookPath, lines.join('\n'), 'utf8');
  const book = await uploadBook(request, user.token, bookPath, {
    chapterPattern: '^Part \\d+',
  });

  // 深链到第二章中段：窗口只含第二章所在 chunk
  await page.goto(`/read/${book.book_id}?locator=50004`);
  const scroller = page.locator('.overflow-y-auto');
  await expect(scroller).toBeVisible();
  await expect(scroller.locator('p').first()).toBeVisible();
  await expect
    .poll(() => minRenderedPos(page), { timeout: 15_000 })
    .toBe(48000);

  // 回到章首贴边，点上区返回上一章：落到第一章章末（尾声行 offset 42000）
  await page.evaluate(() => {
    const el = document.querySelector<HTMLElement>('.overflow-y-auto');
    if (el) el.scrollTop = 0;
  });
  await page.waitForTimeout(200); // 贴边跟踪 rAF
  const { vh, vw } = viewportSize(page);
  await page.mouse.click(vw / 2, vh * 0.05);
  // 章末行在窗口内：确已返回上一章
  await expect
    .poll(() =>
      page.evaluate(() => !!document.querySelector('[data-pos="42000"]')),
    )
    .toBe(true);
  // 修复点：落地窗口自动扩到可滚动。此前窗口只含章末一行（不足一屏、
  // 无 scroll 事件），向上增量加载永不触发，页面卡死在章末几个段落
  await expect
    .poll(
      () =>
        page.evaluate(() => {
          const el = document.querySelector<HTMLElement>('.overflow-y-auto');
          return el ? el.scrollHeight > el.clientHeight : false;
        }),
      { timeout: 10_000 },
    )
    .toBe(true);

  // 向上滚动必须能一路加载到章首（offset 0 的章标题行渲染出来）。
  // 修复前：容器不可滚动，scrollTop 恒 0，窗口永远停在 {7, 8}。
  // 驱动保持在 loadPrev 阈值（<600px）上方递减，模拟真实用户的连续上滚
  for (let i = 0; i < 60; i++) {
    // oxlint-disable-next-line no-await-in-loop
    if ((await minRenderedPos(page)) === 0) break;
    // oxlint-disable-next-line no-await-in-loop
    await page.evaluate(() => {
      const el = document.querySelector<HTMLElement>('.overflow-y-auto');
      if (el) el.scrollTop = Math.max(400, el.scrollTop - 1500);
    });
    // oxlint-disable-next-line no-await-in-loop
    await page.waitForTimeout(120);
  }
  expect(await minRenderedPos(page)).toBe(0);
});
