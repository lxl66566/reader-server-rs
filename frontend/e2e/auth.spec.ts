// 认证冒烟：UI 注册 → 自动登录跳转书库；登出 → 重新登录。
import { expect, test } from '@playwright/test';

import { createUser, seedSession } from './helpers';

test('注册后自动登录并进入书库', async ({ page }) => {
  await page.goto('/register');
  const username = `e2e_reg_${Math.random().toString(36).slice(2, 8)}`;
  await page.getByLabel('用户名').fill(username);
  await page.getByLabel('密码', { exact: true }).fill('pw-e2e-reg');
  await page.getByLabel('确认密码').fill('pw-e2e-reg');
  await page.getByRole('button', { name: '注册' }).click();

  // 注册成功即登录并跳转书库
  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByText('我的书籍')).toBeVisible();
  await expect(
    page.locator('main button', { hasText: '上传书籍' }),
  ).toBeVisible();
});

test('登出后可重新登录', async ({ page }) => {
  const user = await createUser(page.context().request, 'auth');
  seedSession(page, user.token);
  await page.goto('/');
  await expect(page.getByText('我的书籍')).toBeVisible();

  // 登出：header 导航里唯一的 button（移动端文案隐藏，用位置定位）
  await page.locator('header nav button').click();
  const loginLink = page.getByRole('link', { exact: true, name: '登录' });
  await expect(loginLink).toBeVisible();

  // 重新登录
  await loginLink.click();
  await page.getByLabel('用户名').fill(user.username);
  await page.getByLabel('密码', { exact: true }).fill(user.password);
  await page.getByRole('button', { name: '登录' }).click();
  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByText('我的书籍')).toBeVisible();
});
