# reader-server-rs

> 背景：我最初是 [hectorqin/reader](https://github.com/hectorqin/reader) 的用户。由于我受不了其启动时间长、内存占用高、没有阅读时长记录、移动端滑动体验较差、前端页面 bug 多等缺点，我决定自己写一个 Rust 版本的阅读服务器。

reader-server-rs 是一个阅读服务器，用户可以向服务器上传 txt/epub/pdf 书籍，并在任意桌面/移动设备访问网站进行阅读，用户所有的设置与阅读进度都通过服务端无缝同步。特色：

- 高性能与低资源占用：毫秒级启动、常态内存占用 10MB
- 精确的阅读时长记录（活动检测）
- 离线缓存，断网可读
- 完整的后台管理与邀请码机制
- 可向他人分享书籍

由于我的个人习惯，reader-server-rs 不支持：

- 书源功能
- 自定义书籍封面

待改进：书签与批注支持仍然比较粗糙；pdf 支持度偏低

## 安装与使用

从 [Releases](https://github.com/lxl66566/reader-server-rs/releases) 下载 prebuilt binary，然后直接运行即可，默认运行在 localhost:3000，并写入默认配置到同路径的 `config.json` 里。

首次运行请前往 `localhost:3000/admin` 创建管理员帐号，并生成邀请码（或者关闭注册必须使用邀请码的限制）。

NixOS 用户：请使用[我的 NUR](https://github.com/lxl66566/NUR)。

### 配置

```jsonc
{
  "server": {
    "host": "0.0.0.0", // 监听地址
    "port": 3000, // 监听端口
  },
  "db": {
    "url": "sqlite:reader.db", // 数据库连接串，可被环境变量 DATABASE_URL 覆盖
    "max_connections": 10, // 连接池最大连接数
  },
  "storage": {
    "book_dir": "books", // 书籍文件存放目录
    "max_file_size": 52428800, // 上传文件大小上限（字节），默认 50MB
  },
  "jwt": {
    "secret": "", // 留空则首次启动自动生成随机值并写回
    "expiration": 2592000, // 用户 token 有效期（秒），默认 30 天
    "admin_expiration": 604800, // 管理员 token 有效期（秒），默认 7 天
  },
  "log": {
    "dir": "logs", // 日志目录
    "rotation": "never", // 轮转策略："hourly" | "daily" | "never"，never 时仅输出到 stderr
    "level": "info", // 本服务日志等级："trace" | "debug" | "info" | "warn" | "error"；依赖库固定 warn，RUST_LOG 可整体覆盖
  },
  "rate_limit": {
    "per_second": 20, // 每 IP 每秒放行的请求数
    "burst_size": 70, // 令牌桶最大突发容量
    "trust_proxy_headers": false, // 是否信任反代改写的 X-Forwarded-For 等头，仅在反代之后部署时开启
  },
}
```

`config.json` 中未写出的段或字段一律回退默认值。

### 开发

```bash
pnpm dev                         # 同时启动前后端（后端 :3000，前端 :5173，访问 http://localhost:5173）
```

release 构建会自动执行 `pnpm build` 并将 `frontend/dist` 嵌入二进制。
