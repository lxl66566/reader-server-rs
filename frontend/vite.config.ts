import { cpSync, existsSync, readFileSync, statSync } from 'node:fs';
import { resolve } from 'node:path';

import UnoCSS from 'unocss/vite';
import type { Plugin, UserConfig } from 'vite';
import { VitePWA } from 'vite-plugin-pwa';
import solidPlugin from 'vite-plugin-solid';
import { defineConfig, defaultExclude } from 'vitest/config';

// pdf.js 运行时按「目录 + 固定文件名」拉取的静态资源（JBIG2/JPX 的 wasm
// 解码器、CJK CMap、非嵌入字体回退），在 pdfjs-dist 包内整目录提供。
// dev 由中间件伺服、build 拷入 dist，两侧 URL 一致为 /pdfjs-assets/<dir>/。
const PDFJS_ASSETS_PREFIX = '/pdfjs-assets/';
const PDFJS_ASSET_DIRS = ['cmaps', 'standard_fonts', 'wasm'] as const;

const MIME_BY_EXT: Record<string, string> = {
  '.afm': 'text/plain',
  '.bcmap': 'application/octet-stream',
  '.pfb': 'application/octet-stream',
  '.ttf': 'font/ttf',
  '.wasm': 'application/wasm',
};

const pdfjsAssets = (): Plugin => {
  let outDir = 'dist';
  return {
    closeBundle() {
      copyPdfjsAssets(resolve(process.cwd(), outDir));
    },
    configResolved(resolved) {
      outDir = resolved.build.outDir;
    },
    configureServer(server) {
      const packageDir = resolve(process.cwd(), 'node_modules/pdfjs-dist');
      server.middlewares.use((req, res, next) => {
        const pathname = new URL(req.url ?? '/', 'http://localhost').pathname;
        if (!pathname.startsWith(PDFJS_ASSETS_PREFIX)) {
          next();
          return;
        }
        const served = resolvePdfjsAsset(packageDir, pathname);
        if (!served) {
          res.statusCode = 404;
          res.end();
          return;
        }
        res.setHeader(
          'content-type',
          MIME_BY_EXT[served.slice(served.lastIndexOf('.'))] ??
            'application/octet-stream',
        );
        res.end(readFileSync(served));
      });
    },
    name: 'pdfjs-static-assets',
  };
};

function copyPdfjsAssets(distRoot: string): void {
  const packageDir = resolve(process.cwd(), 'node_modules/pdfjs-dist');
  for (const dir of PDFJS_ASSET_DIRS) {
    const from = resolve(packageDir, dir);
    if (existsSync(from)) {
      cpSync(from, resolve(distRoot, `pdfjs-assets/${dir}`), {
        recursive: true,
      });
    }
  }
}

// 仅允许 /pdfjs-assets/<白名单目录>/<单级文件名>，拒绝路径穿越
function resolvePdfjsAsset(
  packageDir: string,
  pathname: string,
): null | string {
  const rel = decodeURIComponent(pathname.slice(PDFJS_ASSETS_PREFIX.length));
  const [dir, file] = rel.split('/');
  if (
    !dir ||
    !file ||
    rel.split('/').length !== 2 ||
    !(PDFJS_ASSET_DIRS as readonly string[]).includes(dir)
  ) {
    return null;
  }
  const root = resolve(packageDir, dir);
  const file_ = resolve(root, file);
  return file_.startsWith(root) &&
    statSync(file_, { throwIfNoEntry: false })?.isFile()
    ? file_
    : null;
}

const config: UserConfig = defineConfig({
  build: {
    target: 'esnext',
  },
  plugins: [
    UnoCSS(),
    solidPlugin(),
    pdfjsAssets(),
    // 离线阅读的外壳：预缓存全部构建产物，断网时页面本身仍可打开；
    // /api 永不进 SW 缓存（数据走 IndexedDB 离线层）。
    // pdf.js 的 wasm 解码器是扫描版 PDF 渲染的硬依赖，一并预缓存；
    // cmaps/standard_fonts 仅影响非嵌入字体文本，按需在线拉取
    VitePWA({
      manifest: false,
      registerType: 'autoUpdate',
      workbox: {
        globPatterns: ['**/*.{js,mjs,css,html,ico,svg,png,woff,woff2,wasm}'],
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
