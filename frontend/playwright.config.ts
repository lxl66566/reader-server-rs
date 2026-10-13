// E2E 配置：目标是已运行的 reader-server（默认 http://127.0.0.1:3000，
// 可用 E2E_BASE_URL 覆盖），本配置不负责启停服务。每个测试经 API 造
// 专属用户，彼此独立；PC 与移动端两个 viewport 项目。
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  expect: { timeout: 10_000 },
  fullyParallel: true,
  outputDir: 'test-results',
  projects: [
    {
      name: 'pc',
      use: {
        ...devices['Desktop Chrome'],
        viewport: { height: 800, width: 1280 },
      },
    },
    {
      name: 'mobile',
      use: {
        ...devices['Pixel 7'],
        viewport: { height: 844, width: 390 },
      },
    },
  ],
  reporter: [['list']],
  retries: process.env.CI ? 1 : 0,
  // 必须显式限定：默认会扫描整个 frontend 目录，把 vitest 的 *.test.ts
  // 也当 e2e 用例加载（其 describe 在 vitest runtime 外调用直接崩溃）
  testDir: './e2e',
  timeout: 60_000,
  use: {
    baseURL: process.env.E2E_BASE_URL ?? 'http://127.0.0.1:3000',
    // 统一界面语言，断言文本用 zh-CN 词典
    locale: 'zh-CN',
    screenshot: 'only-on-failure',
    timezoneId: 'Asia/Shanghai',
    trace: 'retain-on-failure',
    video: 'off',
  },
  // 服务端有全局 IP 限流，并发别拉太高
  workers: 4,
});
