import { fileURLToPath } from 'node:url';

// 阅读设置冒烟：设置页修改字号 → 防抖同步到服务端 → 刷新后保留。
import { expect, test, type Locator } from '@playwright/test';

import { createUser, seedSession, uploadBook } from './helpers';

// 测试书入库于 e2e/assets/（gen-assets.mjs 生成）
const BOOK_PATH = fileURLToPath(new URL('./assets/novel.txt', import.meta.url));

/** 读取步进器当前值 */
async function stepperValue(row: Locator): Promise<number> {
  return Number(await row.locator('span.tabular-nums').innerText());
}

test('修改字号并持久化', async ({ page }) => {
  const user = await createUser(page.context().request, 'settings');
  seedSession(page, user.token);
  await page.goto('/settings');

  // 字号行：SettingRow(label + StepperControl[-, +])
  const fontRow = page.locator('div.py-2', { hasText: '字号' });
  await expect(fontRow).toBeVisible();
  const before = await stepperValue(fontRow);

  await fontRow.getByRole('button').last().click();
  await fontRow.getByRole('button').last().click();
  const after = before + 2;
  await expect(fontRow.locator('span.tabular-nums')).toHaveText(String(after));

  // 设置为 500ms 防抖同步：轮询服务端确认落库
  await expect
    .poll(
      async () => {
        const res = await page
          .context()
          .request.get('/api/v1/reading/settings', {
            headers: { authorization: `Bearer ${user.token}` },
          });
        const { data } = (await res.json()) as {
          data: { font_size: number };
        };
        return data.font_size;
      },
      { intervals: [500], timeout: 10_000 },
    )
    .toBe(after);

  // 刷新后仍为修改值
  await page.reload();
  await expect(fontRow.locator('span.tabular-nums')).toHaveText(String(after));
});

test('新用户首会话直接应用服务端设置', async ({ page }) => {
  // 回归：首登初始化曾只补主题色不应用服务端值，首会话停留在本地
  // 兜底排版（分页模式），第二次打开才变成服务端默认的滚动模式
  const user = await createUser(page.context().request, 'fresh');
  seedSession(page, user.token);
  const book = await uploadBook(page.context().request, user.token, BOOK_PATH);

  await page.goto(`/read/${book.book_id}`);
  // 服务端默认 pagination_mode = scroll：滚动容器应直接出现，
  // 且不出现分页指示器（"{page} / {total}"）
  await expect(page.locator('.overflow-y-auto')).toBeVisible();
  await expect(page.getByText(/^\d+ \/ \d+$/)).toHaveCount(0);
});
