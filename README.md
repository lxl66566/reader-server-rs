# reader-server-rs

这是 [hectorqin/reader](https://github.com/hectorqin/reader) 的 Rust 精简版。由于我受不了原版启动时间长、没有阅读时长记录、移动端滑动体验较差等缺点，我决定自己写一个 Rust 版本的。出于我个人的习惯，reader-server-rs 砍掉了书源功能。

reader-server-rs 是一个阅读服务器，用户可以向服务器上传 txt 书籍，并在任意设备的网页上进行阅读，用户所有的设置与阅读进度通过服务端同步。

## 安装与使用

从 [Releases](https://github.com/lxl66566/reader-server-rs/releases) 中下载 prebuilt binary 即可。

然后需要在同目录创建一个 config.json:

```json
{
  "server": {
    "host": "0.0.0.0",
    "port": 3000
  },
  "db": {
    "url": "sqlite://./data/reader.db",
    "max_connections": 10
  },
  "storage": {
    "book_dir": "books",
    "max_file_size": 52428800
  },
  "jwt": {
    "secret": "cbd68e0e-3b32-4607-8092-9a17a83a3b1a", // 必须，需要自己生成一个
    "expiration": 2592000,
    "admin_expiration": 604800
  },
  "log": {
    "dir": "logs",
    "rotation": "daily"
  },
  "rate_limit": {
    "per_second": 100000,
    "burst_size": 300000
  }
}
```

然后直接运行即可，默认运行在 localhost:3000。

首次运行请前往 localhost:3000/admin 创建管理员帐号，并生成邀请码（或者关闭注册必须使用邀请码的限制）。

NixOS 用户：请使用[我的 NUR](https://github.com/lxl66566/NUR)。

### 离线阅读

阅读器顶栏的离线缓存按钮支持缓存后续若干章或缓存全本（txt 按分块、epub/pdf 按整本文件），阅读过的 txt 内容也会自动写穿到本地缓存。断网后重新打开页面即可继续阅读：页面外壳由 Service Worker 预缓存提供，书籍内容与详情、书架、书签、批注、阅读设置均回退到 IndexedDB 中的本地副本；离线期间产生的阅读进度会在恢复联网后随心跳自动推送。缓存按用户隔离，可在缓存面板中随时清除。

离线打开页面依赖 Service Worker，需要 secure context（HTTPS 或 localhost）；纯 HTTP 的局域网部署无法使用离线打开，但已打开的页面在断网后仍可继续阅读与缓存已加载的内容。

### 开发

前端由 vite dev server 单独提供，后端不内嵌：

```bash
pnpm dev                         # 同时启动前后端（后端 :3000，前端 :5173，访问 http://localhost:5173）
```

### 发布

release 构建会自动执行 `pnpm build` 并将 `frontend/dist` 嵌入二进制（可用 `SKIP_FRONTEND_BUILD=1` 跳过自动打包）：

```bash
cargo build --release
./target/release/reader-server   # 访问 http://127.0.0.1:3000
```
