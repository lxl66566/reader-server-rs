// 导航栏主题切换（ThemeSwitcher）冒烟：游客本地持久化、跟随系统、
// 登录后以服务端为准、登录态切换同步服务端、退出后保留本地主题。
import { expect, test, type Page } from '@playwright/test';

import {
  createUser,
  getSettings,
  seedSession,
  updateSettings,
} from './helpers';

/** 按钮只显示当前模式，选择项在点击展开的菜单里 */
async function selectTheme(page: Page, label: string) {
  await themeButton(page).click();
  await page.getByRole('menuitemradio', { name: label }).click();
}

/** 定位导航栏主题按钮（aria-label 为 zh-CN 词典文案） */
function themeButton(page: Page) {
  return page.locator('nav').getByRole('button', { name: '主题' });
}

test('游客可切换主题并本地持久化，跟随系统', async ({ page }) => {
  await page.goto('/');
  // 默认跟随系统：Playwright colorScheme=light
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');

  await selectTheme(page, '深色');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  expect(await page.evaluate(() => localStorage.getItem('rs_theme'))).toBe(
    'dark',
  );

  // 刷新后仍取本地存储的主题
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');

  // 跟随系统：随 prefers-color-scheme 实时翻转
  await selectTheme(page, '跟随系统');
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.emulateMedia({ colorScheme: 'light' });
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
});

test('登录后以服务端主题为准，切换同步服务端，退出保留本地', async ({
  page,
}) => {
  const request = page.context().request;
  const user = await createUser(request, 'theme');
  await updateSettings(request, user.token, { theme: 'sepia' });
  // 本地预置 dark，验证登录加载后被服务端 sepia 覆盖
  await page.addInitScript(() => {
    localStorage.setItem('rs_theme', 'dark');
  });
  seedSession(page, user.token);

  await page.goto('/');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'sepia');

  // 登录态切换：UI 立即生效，防抖后同步服务端
  await selectTheme(page, '深色');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await expect
    .poll(
      async () => {
        const settings = await getSettings(request, user.token);
        return settings.theme;
      },
      {
        intervals: [500],
        timeout: 10_000,
      },
    )
    .toBe('dark');

  // nav 内最后一个 button 是退出（移动端标签隐藏，不能按名称定位）
  await page.locator('nav').getByRole('button').last().click();
  // 退出后主题不被重置，仍保留在 localstorage
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  expect(await page.evaluate(() => localStorage.getItem('rs_theme'))).toBe(
    'dark',
  );
});
