import UnoCSS from 'unocss/vite';
import type { UserConfig } from 'vite';
import { VitePWA } from 'vite-plugin-pwa';
import solidPlugin from 'vite-plugin-solid';
import { defineConfig, defaultExclude } from 'vitest/config';

const config: UserConfig = defineConfig({
  build: {
    target: 'esnext',
  },
  plugins: [
    UnoCSS(),
    solidPlugin(),
    // 离线阅读的外壳：预缓存全部构建产物，断网时页面本身仍可打开；
    // /api 永不进 SW 缓存（数据走 IndexedDB 离线层）
    VitePWA({
      manifest: false,
      registerType: 'autoUpdate',
      workbox: {
        globPatterns: ['**/*.{js,mjs,css,html,ico,svg,png,woff,woff2}'],
        maximumFileSizeToCacheInBytes: 12 * 1024 * 1024,
        navigateFallback: 'index.html',
        navigateFallbackDenylist: [/^\/api\//],
      },
    }),
  ],
  server: {
    host: '0.0.0.0',
    port: 5173,
    proxy: {
      // backend listens on :3000; ws upgrade needs ws: true
      '/api': {
        target: process.env.E2E_PROXY ?? 'http://127.0.0.1:3000',
        ws: true,
      },
      '/health': {
        target: process.env.E2E_PROXY ?? 'http://127.0.0.1:3000',
      },
    },
  },
  test: {
    environment: 'node',
    // e2e 目录的 *.spec.ts 由 playwright test 驱动，不进 vitest
    exclude: [...defaultExclude, 'e2e/**'],
  },
});

export default config;
