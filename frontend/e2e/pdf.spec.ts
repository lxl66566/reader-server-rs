// PDF 冒烟：样本书（e2e/assets/sample.pdf，gen-assets.mjs 生成，12 页 +
// UTF-16BE 元数据 + 两层 outline 书签）验证画布渲染、两种翻页模式、
// 目录/书签跳转、进度落库与恢复。不依赖任何真实书籍文件。
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
const SAMPLE_PDF = fileURLToPath(
  new URL('./assets/sample.pdf', import.meta.url),
);
const TOTAL_PAGES = 12;

const indicator = (page: Page) =>
  page.getByText(`1 / ${TOTAL_PAGES}`, { exact: true });

/** 已挂载的 canvas 数（滚动模式惰性渲染应远小于总页数） */
async function canvasCount(page: Page): Promise<number> {
  return page.evaluate(() => document.querySelectorAll('canvas').length);
}

/** 首个 canvas 是否已绘制非纯白内容（pdf.js 渲染成功而非空白页） */
async function canvasPainted(page: Page): Promise<boolean> {
  return page.evaluate(() => {
    const canvas = document.querySelector('canvas');
    if (!canvas || canvas.width === 0) return false;
    const ctx = canvas.getContext('2d');
    if (!ctx) return false;
    const { data } = ctx.getImageData(0, 0, canvas.width, canvas.height);
    for (let i = 0; i < data.length; i += 4) {
      // noUncheckedIndexedAccess：越界按白色处理（视为未绘制）
      const red = data[i] ?? 255;
      const green = data[i + 1] ?? 255;
      const blue = data[i + 2] ?? 255;
      if (red < 250 || green < 250 || blue < 250) return true;
    }
    return false;
  });
}

/** 页码指示器文本（翻页模式） */
async function indicatorText(page: Page): Promise<string> {
  return page.evaluate(() => {
    const el = [...document.querySelectorAll('div')].find(d =>
      /^\d+ \/ \d+$/.test(d.textContent.trim()),
    );
    return el === undefined ? '' : el.textContent.trim();
  });
}

/** 打开阅读页直到画布渲染；限流瞬断出现“加载失败”面板时点重试自愈 */
async function readerPainted(page: Page): Promise<void> {
  await expect
    .poll(
      async () => {
        const retry = page.getByRole('button', { name: '重试' });
        if (await retry.isVisible()) await retry.click();
        return canvasPainted(page);
      },
      { intervals: [500, 1000], timeout: 30_000 },
    )
    .toBe(true);
}

async function scrollerTop(page: Page): Promise<number> {
  return page.evaluate(() => {
    const el = [...document.querySelectorAll('div')].find(d =>
      d.className.includes('overflow-y-auto'),
    );
    return el ? Math.round(el.scrollTop) : -1;
  });
}

/** 起跑错峰：4 个 worker 同时建号+上传+开页会触发服务端全局限流（10 rps） */
async function stagger(): Promise<void> {
  await new Promise(resolve => {
    setTimeout(resolve, Math.floor(Math.random() * 2500));
  });
}

function viewportSize(page: Page): { vh: number; vw: number } {
  const size = page.viewportSize();
  return { vh: size?.height ?? 800, vw: size?.width ?? 1280 };
}

test('pdf 打开渲染非空白、翻页与进度落库恢复', async ({ page }) => {
  await stagger();
  const user = await createUser(page.context().request, 'pdfread');
  seedSession(page, user.token);
  const request = page.context().request;
  await updateSettings(request, user.token, { pagination_mode: 'paginate' });
  // 回归：中文 PDF 的标题普遍存为 UTF-16BE，解码失败会得到乱码书名
  const book = await uploadBook(request, user.token, SAMPLE_PDF);
  expect(book.title).toBe('测试PDF样书');
  expect(book.total_pages).toBe(TOTAL_PAGES);

  await page.goto(`/read/${book.book_id}`);
  await readerPainted(page);
  await expect(indicator(page)).toBeVisible();

  // 键盘翻页（page down 前进）→ 右区 tap 前进、左区回退
  await page.keyboard.press('PageDown');
  await expect.poll(() => indicatorText(page)).toBe(`2 / ${TOTAL_PAGES}`);
  const { vh, vw } = viewportSize(page);
  await page.mouse.click(vw * 0.9, vh / 2);
  await expect.poll(() => indicatorText(page)).toBe(`3 / ${TOTAL_PAGES}`);
  await page.mouse.click(vw * 0.1, vh / 2);
  await expect.poll(() => indicatorText(page)).toBe(`2 / ${TOTAL_PAGES}`);

  // 中区 tap 唤出 chrome
  await page.mouse.click(vw / 2, vh / 2);
  await expect(page.getByRole('button', { name: '目录' })).toBeVisible();
  await page.mouse.click(vw - 5, vh / 2);
  await expect(page.getByRole('button', { name: '目录' })).toBeHidden();

  // 心跳把当前页写回服务端：等 percent 收敛到第 2 页的精确值再重开，
  // 避免读到中途心跳导致恢复页漂移
  await expect
    .poll(
      async () => (await getBook(request, user.token, book.book_id)).percent,
      { intervals: [1000], timeout: 20_000 },
    )
    .toBeCloseTo(1 / (TOTAL_PAGES - 1), 3);

  // 重开恢复到已保存页
  await page.reload();
  await readerPainted(page);
  await expect.poll(() => indicatorText(page)).toBe(`2 / ${TOTAL_PAGES}`);
});

test('pdf 目录（outline 书签）跳转与书签跳回', async ({ page }) => {
  await stagger();
  const user = await createUser(page.context().request, 'pdftoc');
  seedSession(page, user.token);
  const request = page.context().request;
  await updateSettings(request, user.token, { pagination_mode: 'paginate' });
  const book = await uploadBook(request, user.token, SAMPLE_PDF);

  await page.goto(`/read/${book.book_id}`);
  await readerPainted(page);
  const { vh, vw } = viewportSize(page);
  await page.mouse.click(vw / 2, vh / 2);
  await page.getByRole('button', { name: '目录' }).click();

  // outline 条目（含嵌套「小节」）：跳到最后一页。jump 只关弹窗，
  // chrome 仍开着——先点遮罩关闭，中区 tap 才能重新唤出 chrome
  await page.getByRole('button', { name: 'Appendix' }).click();
  await expect
    .poll(() => indicatorText(page))
    .toBe(`${TOTAL_PAGES} / ${TOTAL_PAGES}`);
  await page.mouse.click(vw - 5, vh / 2);
  await expect(page.getByRole('button', { name: '目录' })).toBeHidden();

  // 书签：在当前页添加后翻走，再从书签跳回
  await page.mouse.click(vw / 2, vh / 2);
  await page.getByRole('button', { name: '书签' }).click();
  await page.getByPlaceholder('书签备注（选填）').fill('标记');
  await page.getByRole('button', { name: '添加书签' }).click();
  await expect(page.getByText('已添加书签')).toBeVisible();
  await page.mouse.click(vw - 5, vh / 2); // 关弹窗
  await page.mouse.click(vw - 5, vh / 2); // 关 chrome
  await page.keyboard.press('PageUp');
  await expect.poll(() => indicatorText(page)).toBe(`11 / ${TOTAL_PAGES}`);

  await page.mouse.click(vw / 2, vh / 2);
  await page.getByRole('button', { name: '书签' }).click();
  await page.getByRole('button', { name: '标记' }).click();
  await expect
    .poll(() => indicatorText(page))
    .toBe(`${TOTAL_PAGES} / ${TOTAL_PAGES}`);
  await page.mouse.click(vw - 5, vh / 2);
  await expect(page.getByRole('button', { name: '目录' })).toBeHidden();

  // 嵌套子条目跳到第 2 页
  await page.mouse.click(vw / 2, vh / 2);
  await page.getByRole('button', { name: '目录' }).click();
  await page.getByRole('button', { name: '小节' }).click();
  await expect.poll(() => indicatorText(page)).toBe(`2 / ${TOTAL_PAGES}`);
});

test('pdf 滚动模式 tap 分区滚动、惰性渲染与进度上报', async ({ page }) => {
  await stagger();
  const user = await createUser(page.context().request, 'pdfscroll');
  seedSession(page, user.token);
  const request = page.context().request;
  await updateSettings(request, user.token, { pagination_mode: 'scroll' });
  const book = await uploadBook(request, user.token, SAMPLE_PDF);

  await page.goto(`/read/${book.book_id}`);
  await readerPainted(page);
  const { vh, vw } = viewportSize(page);

  // 中区唤出菜单、下区滚动 90% 视口
  await page.mouse.click(vw / 2, vh / 2);
  await expect(page.getByRole('button', { name: '目录' })).toBeVisible();
  await page.mouse.click(vw - 5, vh / 2);
  await expect(page.getByRole('button', { name: '目录' })).toBeHidden();
  await page.mouse.click(vw / 2, vh * 0.9);
  await expect.poll(() => scrollerTop(page)).toBeGreaterThan(0);

  // 惰性渲染：页列很长，但已挂载的画布远小于总页数
  await expect.poll(() => canvasCount(page)).toBeLessThan(TOTAL_PAGES);

  // 滚到底：进度上报收敛到书尾
  await expect
    .poll(
      async () => {
        await page.mouse.wheel(0, vh * 2);
        await page.waitForTimeout(200);
        return (await getBook(request, user.token, book.book_id)).percent;
      },
      { intervals: [500, 1000], timeout: 25_000 },
    )
    .toBeGreaterThan(0.8);
  // 滚动过程中画布按需挂载/释放，不会把 12 页全部常驻
  expect(await canvasCount(page)).toBeLessThan(TOTAL_PAGES);
});
