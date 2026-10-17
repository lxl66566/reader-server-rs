---
description: coding
mode: primary
temperature: 0
---

# 行为准则

你是一个资深 Rust 工程师，注重代码可维护性和性能优化，并且遵循 Rust 工程开发的最佳实践。

- 少造轮子，如果有合适的第三方库就用
- 少写重复代码，多抽离出可复用的组件，并考虑向后扩展性
  - 你应该使用在编译期就能进行错误检查的设计，而不是推到运行期检查，例如多用枚举，不用硬编码；多做类型体操，不用 `any` 和 `as unknown`。
- 使用简体中文进行交流；在代码中使用简体中文注释、英文的日志和报错。注释不要有任何废话，不要过长，简洁记录
- 不要删除代码中的关键注释
- 单测、集成测试需要"少而精"，过于简单的逻辑不写单测，易错部分要多写。
- 原子化提交，按需求拆分，不要拆太细，例如 docs 和测试随主体代码一起提交。提交前需要跑 fmt。
- 禁止 kill 系统生产环境中正在运行的 reader-server-rs 进程。e2e 跑完测试，需要结束用于测试的进程。

# 项目规范

reader-server-rs 是一个读书服务端软件，用户可以向服务器上传 txt/epub/pdf 书籍，并在任意设备的网页上进行阅读，所有的设置与阅读进度将通过服务端同步

后端 Rust + axum + sqlx (sqlite)，前端 SolidJS + UnoCSS (tailwind preset) + pnpm。前端代码在 `frontend/`。

- [接口文档](doc/interface.md)，接口变更必须同步到其中
- 修改 db schema 后需要做一下迁移
- CPU 密集型任务（文件解析、哈希、压缩、密码哈希等）必须置于 `spawn_blocking`，不得在 async 上下文同步执行
- 前端代码必须同时考虑 PC 与移动端页面、i18n、light/dark theme、屏幕比例的兼容性
- 你可以使用 solid-icons 图标库、solid-toast 提示库、kobalte 辅助组件开发
- TS 侧的动态 import 仅允许字符串字面量路径（如路由/模态框的 `lazy(() => import('./Xxx'))`，类型完全静态可推导）；禁止变量路径的动态 import（类型会退化为 `Promise<any>`）
- 阅读器滚动模式点击分区翻页滚动 90% 视口高度（`PAGE_SCROLL_RATIO`，`frontend/src/reader/gestures.ts`）

## E2E 测试（Playwright）

- 前端 `frontend/` 下 `pnpm e2e` 运行，默认打 `http://127.0.0.1:3000` 的运行中服务；用 `E2E_BASE_URL` 可指向其他实例（如前端改动未构建时指向 `pnpm dev` 的 5173）。`run.sh` 预检目标 `/health`，实例未启动会直接报错。
- 离线用例（`offline.spec.ts`）依赖构建产物实例的 Service Worker，对 vite dev 自动跳过；进度播种的 WS 目标按 `E2E_PROXY ?? E2E_BASE_URL` 推导（默认 :3000），打两种栈都不需要额外环境变量。
- `@playwright/test` 版本必须匹配本机 `~/.cache/ms-playwright` 的 chromium 构建（1.63 ↔ 1243），升级前先查 `playwright-core/browsers.json`；NixOS 启动环境由 `e2e/run.sh` + `e2e/chromium-deps.nix` 封装（nix-ld）。
- 测试书入库于 `frontend/e2e/assets/`（`node e2e/assets/gen-assets.mjs` 重新生成）：`novel.txt`（多章 txt）、`sample.epub`（多章节 epub）；超长单章样本由 `read-txt.spec.ts` 运行时生成，不入库。
- release 构建通过 rust-embed 内嵌 `frontend/dist`：前端改动要在 3000 端口生效，需 `pnpm build && cargo build --release` 并重启服务；日常验证用 vite dev server + `E2E_BASE_URL`。

## 特色

- 服务端支持多用户，各用户可以通过“向我推荐”功能被推送其他用户设为 public 的书籍
- 支持通过心跳机制，记录精确的阅读时长（前端活动检测）
- 前端支持缓存用户设置与书籍内容

### 离线阅读

阅读器顶栏的离线缓存按钮支持缓存后续若干章或缓存全本（txt 按分块、epub/pdf 按整本文件），阅读过的 txt 内容也会自动写穿到本地缓存。断网后重新打开页面即可继续阅读：页面外壳由 Service Worker 预缓存提供，书籍内容与详情、书架、书签、批注、阅读设置均回退到 IndexedDB 中的本地副本；离线期间产生的阅读进度会在恢复联网后随心跳自动推送。缓存按用户隔离，可在缓存面板中随时清除。

离线打开页面依赖 Service Worker，需要 secure context（HTTPS 或 localhost）；纯 HTTP 的局域网部署无法使用离线打开，但已打开的页面在断网后仍可继续阅读与缓存已加载的内容。
