# 后端功能总结与前端实施计划

> 本文档由对 `src/`（Rust 后端）与 `doc/interface.md` 的探索生成，用于指导前端开发。

## 1. 后端已实现的功能

### 认证与用户（全部已实现）

- 认证方式：**无状态 JWT（HS256）**，请求头 `Authorization: Bearer <token>`，无 cookie/session。
- 用户 token 有效期 30 天，管理员 token 7 天（独立 admin 体系，`role` 区分）。
- `token_version` 吊销机制：管理员吊销/封禁后旧 token 立即失效。
- 登录失败 5 次锁定 15 分钟；注册可强制邀请码（全局开关 + 邀请码次数限制）。
- 接口：register / login / user_info / change_password / delete_account（密码确认，级联删除）
  / export（zip：metadata.json + 全部书籍原文件）/ admin setup（仅首次）/ admin login。
- **无登出接口**：前端登出 = 本地丢弃 token。

### 书籍（全部已实现）

- 上传：multipart（file/title/author/is_public），**同步处理**，服务端按内容自动识别
  txt / epub / pdf（不依赖扩展名）；大小上限默认 50MB（config.json 可调）。
- txt 章节按正则分割（position = 全文字符偏移）；epub 按 spine 顺序
  （position = spine 索引，0-based，对齐 epub.js）；**pdf 不提取章节**（未实现 outline）。
- 列表 / 详情（含 chapters）/ 搜索（title、author、章节标题，scope = mine|public|all）
  / 部分更新 / 删除（仅 owner）/ 公开书列表 / 随机推荐 `random_public`（"向我推荐"）。
- `/file`：原文件流，强 ETag 支持 304（**不支持 HTTP Range**）。
- `/content?position&length`：**仅 txt**，按字符偏移流式返回 `{ content, next_position }`，
  弱 ETag 支持 304。
- `/jump_to_chapter`：返回章节 position（txt=字符偏移，epub=spine 索引）。
- 标签：GET / PUT（覆盖式，仅 owner）。

### 阅读进度与心跳（WebSocket，已实现）

- `GET /api/v1/reading/ws`，浏览器用 query 参数 `?token=<jwt>` 鉴权。
- 协议：客户端 `subscribe{book_id, device_id}` → `heartbeat{book_id, locator, percent}`
  → `unsubscribe{book_id}`；必须先 subscribe 才能 heartbeat。
- 服务端推送：`subscribed` / `heartbeat_ack{synced, locator, percent, reading_time}`
  / `progress_update`（同账号另一设备更新，本设备应跳转）
  / `bookmark_changed` / `annotation_changed`（另一端增删改，收到后重新拉取）/ `error`。
- 设备切换逻辑：心跳的 device 与服务端 `last_device_id` 不同 → `synced=false`，
  返回服务端进度，不更新；客户端应跳转到返回位置。
- 阅读时长：相邻心跳间隔 ≤30s 才计入；心跳间隔建议 1s–30s。服务端 30s ping，
  120s 无响应断开（前端需重连）。
- 阅读时长/连续天数按用户设置的 timezone 归算。

### 阅读设置与统计（已实现）

- `GET/PUT /api/v1/reading/settings`，13 个字段：font_size、background_color、text_color、
  line_height、letter_spacing、paragraph_spacing、reading_width、text_indent、
  chinese_conversion（none|simplified|traditional）、timezone、font_family、theme（light|sepia|dark|system）、
  pagination_mode（scroll|paginate）。GET 支持 ETag/304；PUT 为部分更新。
- `GET /api/v1/reading/stats`：总时长、当前/最长连续天数、近 30 天每日时长。

### 书签与批注（已实现）

- 书签：list / create（locator+percent+label）/ delete。
- 批注：kind = note | highlight；highlight 必须 end_locator/end_percent 且 end > start；
  list / create / update（excerpt/content）/ delete。
- 写接口可带 `X-Device-Id` 头，避免自己收到 ws 回环广播。

### 管理员（已实现）

- check_setup（判断是否首次）/ 邀请码增查 / 全局设置（invite_code_required）
  / 用户列表、重置密码、吊销 token、封禁/解封 / 管理员增查 / 审计日志（分页）
  / 全局备份 zip（db + books 目录）。

## 2. 后端未实现 / 前端需注意

| 项 | 状态 | 前端对策 |
|---|---|---|
| PDF 章节 / outline | 后端不解析 | pdf.js 自行读取 outline，或仅支持页码跳转 |
| 封面上传接口 | 无（仅有 cover_path 字段） | 不做封面上传，书库用纯色/首字占位 |
| 书内全文搜索（FTS） | 无 | 不做，搜索仅限元数据 |
| 登出接口 | 无 | 本地清 token |
| HTTP Range | 刻意不支持 | `/file` 一次性整取 |
| 前端托管 | 后端不 serve 静态文件，CORS 全放开 | 开发用 vite proxy 到 :3000；生产另行部署 |
| 错误格式 | HTTP 状态码 + body `message`；`code` 恒 0 | 按状态码 + message 处理，勿依赖 code |
| 限流 | 按 IP 10rps/burst30，超限 429 | 请求去抖；429 时提示稍后重试 |

## 3. locator / percent 约定（前端核心）

| 格式 | locator | percent | 内容获取 |
|---|---|---|---|
| txt | 全文字符偏移（字符串数字） | position / total_chars | `/content` 分页拉取，本地缓存 |
| epub | EpubCFI | cfi 进度（epub.js locations） | `/file` + epub.js 渲染 |
| pdf | `page:N`（0 起） | page / total_pages | `/file` + pdf.js 渲染 |

percent 一律由前端计算上报；后端只校验范围并同步。

## 4. 前端实施计划

### 技术选型

- 路由 `@solidjs/router`；提示 `solid-toast`；图标 `solid-icons`。
- 简繁转换 `opencc-js`（客户端，对应 chinese_conversion 设置，none 时不转换）。
- epub 渲染 `epubjs`；pdf 渲染 `pdfjs-dist`（懒加载，仅进入对应书籍时引入）。
- 请求层：自封装 fetch（token 注入、401 跳转、ETag 缓存、429 提示）。
- 状态：solid-js context/store；i18n：字典式（zh-CN / en），light/dark/sepia/system 主题
  走 CSS 变量 + `data-theme`。

### 页面

1. 首页 `/`：未登录先展示产品特色 + 登录引导（设计要点 4）；登录后为书库
   （我的书、搜索、上传、公开书库、向我推荐、阅读统计入口）。
2. 登录 `/login`、注册 `/register`（邀请码按需）。
3. 书籍详情 `/book/:id`：信息、章节目录、书签、批注、标签、编辑/删除、开始阅读。
4. 阅读页 `/read/:id`：按 format 分 TxtReader / EpubReader / PdfReader；
   共用 ReaderShell（顶栏、目录、书签/批注面板、设置面板）+ ws 心跳 hook
   （device_id 持久化 localStorage、synced=false/ progress_update 时跳转）。
   txt 支持 scroll / paginate 两种模式；移动端左右滑动翻页 + 翻页动画、上下滚动；
   点按左右/中间区域翻页/唤出菜单。
5. 设置 `/settings`：阅读设置（实时预览）+ 账户（改密码、导出、注销）。
6. 统计 `/stats`：总时长、连续天数、近 30 天柱状图。
7. 管理页 `/admin`：首次 setup 引导；登录后 dashboard（邀请码、全局开关、用户管理、
   管理员、审计日志、备份下载）。风格从简。

### 设计基调

简洁质朴：纸感背景、窄阅读栏、无渐变无卡片阴影堆砌；动画仅翻页/淡入；移动端优先
（触控区、手势、安全区）。
