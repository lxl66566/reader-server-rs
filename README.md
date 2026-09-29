# reader-server-rs

WIP

这是 [hectorqin/reader](https://github.com/hectorqin/reader) 的 Rust 精简版。由于我受不了原版启动时间长、没有阅读时长记录、移动端滑动体验较差等缺点，我决定自己写一个 Rust 版本的。出于我个人的习惯，reader-server-rs 砍掉了书源功能。

reader-server-rs 是一个阅读服务器，用户可以向服务器上传 txt 书籍，并在任意设备的网页上进行阅读，用户所有的设置与阅读进度通过服务端同步。

## 运行

### 开发

前端由 vite dev server 单独提供，后端不内嵌：

```bash
cargo run                        # 后端 :3000（仅 API）
cd frontend && pnpm dev          # 前端 :5173，访问 http://localhost:5173
```

### 发布

release 构建会自动执行 `pnpm build` 并将 `frontend/dist` 嵌入二进制（可用 `SKIP_FRONTEND_BUILD=1` 跳过自动打包）：

```bash
cargo build --release
./target/release/reader-server   # 访问 http://127.0.0.1:3000
```
