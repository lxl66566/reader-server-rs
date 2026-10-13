import UnoCSS from 'unocss/vite';
import type { UserConfig } from 'vite';
import solidPlugin from 'vite-plugin-solid';
import { defineConfig, defaultExclude } from 'vitest/config';

const config: UserConfig = defineConfig({
  build: {
    target: 'esnext',
  },
  plugins: [UnoCSS(), solidPlugin()],
  server: {
    host: '0.0.0.0',
    port: 5173,
    proxy: {
      // backend listens on :3000; ws upgrade needs ws: true
      '/api': {
        target: 'http://127.0.0.1:3000',
        ws: true,
      },
      '/health': {
        target: 'http://127.0.0.1:3000',
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
