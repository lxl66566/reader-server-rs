# 错误响应

业务错误统一通过 HTTP 状态码 + 响应体 `message` 字段返回；`code` 字段恒为 `0`（保留位，未启用）。前端应基于 HTTP 状态码（401/403/404/400/500）和 `message` 文本做错误分支，不要依赖数字错误码。

| HTTP status | 触发场景（典型） |
|---|---|
| 400 | 参数校验失败（用户名长度、密码长度、邀请码、上传文件格式/大小、批注范围、位置越界、无效时区、无效搜索 scope 等） |
| 401 | 未登录 / token 过期 / 账户被锁定 / 账户被ban / token 角色不匹配 |
| 403 | 无权限访问 / 修改 / 删除目标资源（如非 owner 操作他人书） |
| 404 | 资源不存在（书 / 章节 / 用户 / 书签 / 批注 / 管理员） |
| 429 | 限流（按 IP，默认 10 rps / burst 30） |
| 500 | 数据库 / IO / 内部错误（响应文案统一为 "Internal server error"，详情见服务端日志） |

> 所有业务接口前缀统一为 `/api/v1`；健康检查 `/health` 不在版本前缀下。
> 文中所有 `position` / `start_position` / `end_position` / 章节 `position` / `next_position` 单位均为"全文字符偏移（`chars().count()`）"，与上传时持久化的 `total_chars` 保持一致，前端可据此计算阅读百分比（`position / total_chars`）。

# 通用类型与认证

```
type ApiResponse<T> = { code: number; message: string; data: T }

type ReadingSettings = {
  font_size: number; background_color: string; text_color: string;
  line_height: number; letter_spacing: number; paragraph_spacing: number;
  reading_width: number; text_indent: number; simplified_chinese: boolean;
  timezone: string;
  font_family: string;
  theme: 'light' | 'sepia' | 'dark' | 'system';
  pagination_mode: 'scroll' | 'paginate'
}

type Book = {
  book_id: number; title: string; author: string | null; is_public: boolean;
  created_at: string; last_read_at: string | null; position: number; reading_time: number;
  total_chars: number; description: string | null
}
type BookDetail = Book & { chapters: { chapter_id: number; title: string; position: number }[] }
type PublicBook = {
  book_id: number; title: string; author: string | null; owner_username: string;
  created_at: string; total_chars: number; description: string | null
}
type InviteCodeItem = { id: number; invite_code: string; limit_times: number; used_times: number; description: string | null; created_at: string }
type UserItem = { user_id: number; username: string; created_at: string; book_count: number; total_reading_time: number; status: 'active' | 'banned'; banned_at: string | null }
type AdminItem = { admin_id: number; username: string; created_at: string }
type AuditLogItem = { id: number; admin_id: number; admin_username: string; action: string; target_type: string | null; target_id: number | null; detail: string | null; created_at: string }
type Bookmark = { id: number; user_id: number; book_id: number; position: number; label: string | null; created_at: string }
type Annotation = {
  id: number; user_id: number; book_id: number; kind: 'note' | 'highlight';
  start_position: number; end_position: number | null; excerpt: string | null;
  content: string | null; created_at: string; updated_at: string
}
```

`Auth`  = 请求头 `Authorization: Bearer {token}`
`Admin` = 请求头 `Authorization: Bearer {admin_token}`

# 接口列表

### 用户认证

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| GET | /health | - | - | 文本 "ok" |
| POST | /api/v1/auth/register | - | Body: { username: string; password: string; invite_code?: string } | { user_id: number; username: string; token: string } |

> 全局按客户端 IP 维度限流，默认 10 rps / burst 30；超限返回 429。
> `/health` 与带 If-None-Match 命中的请求不消耗带宽。
| POST | /api/v1/auth/login | - | Body: { username: string; password: string } | { user_id: number; username: string; token: string } |
| GET | /api/v1/auth/user_info | Auth | - | { user_id: number; username: string; total_reading_time: number; book_count: number; status: 'active' \| 'banned' } |
| POST | /api/v1/auth/change_password | Auth | Body: { old_password: string; new_password: string } | {} |
| DELETE | /api/v1/auth/account | Auth | Body: { password: string } | {} |
| GET | /api/v1/auth/export | Auth | - | application/zip 流，文件名 `<username>_<timestamp>.zip`，内含 metadata.json（书/章节/进度/设置/书签/批注）+ 所有原始 txt 文件 |
| POST | /api/v1/auth/admin/setup | - | Body: { username: string; password: string } | { admin_token: string } |
| POST | /api/v1/auth/admin/login | - | Body: { username: string; password: string } | { admin_token: string } |

### 管理员

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| GET | /api/v1/admin/check_setup | - | - | { is_setup: boolean } |
| POST | /api/v1/admin/invite_code | Admin | Body: { limit_times: number; description?: string } | { invite_code: string; limit_times: number; description: string \| null } |
| GET | /api/v1/admin/invite_codes | Admin | - | { invite_codes: InviteCodeItem[] } |
| GET | /api/v1/admin/settings | Admin | - | { invite_code_required: boolean } |
| PUT | /api/v1/admin/settings | Admin | Body: { invite_code_required: boolean } | {} |
| GET | /api/v1/admin/users | Admin | - | { users: UserItem[] } |
| POST | /api/v1/admin/users/:user_id/reset_password | Admin | Body: { new_password: string } | {} |
| POST | /api/v1/admin/users/:user_id/revoke_tokens | Admin | - | {} |
| POST | /api/v1/admin/users/:user_id/ban | Admin | - | {} |
| POST | /api/v1/admin/users/:user_id/unban | Admin | - | {} |
| GET | /api/v1/admin/admins | Admin | - | { admins: AdminItem[] } |
| POST | /api/v1/admin/admins | Admin | Body: { username: string; password: string } | {} |
| GET | /api/v1/admin/audit_logs?page=number&limit=number | Admin | - | { total: number; logs: AuditLogItem[] } |
| GET | /api/v1/admin/backup | Admin | - | application/zip 流，包含 reader.db（已 WAL checkpoint）+ 整个 books 目录（不压缩），用于全局灾难恢复 |

### 书籍管理

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| POST | /api/v1/books/upload | Auth | FormData: file (txt/epub), title: string, author?: string, is_public: boolean | { book_id: number; title: string; author: string \| null; chapters: { chapter_id: number; title: string; position: number }[] } |
| GET | /api/v1/books?page=number&limit=number | Auth | - | { total: number; books: Book[] } |
| GET | /api/v1/books/search?q=string&scope=mine\|public\|all&page=number&limit=number | Auth | - | { total: number; books: Book[] } |
| GET | /api/v1/books/:book_id | Auth | - | BookDetail |
| GET | /api/v1/books/:book_id/content?position=number&length=number | Auth | Header: If-None-Match? | 200 + ETag：{ content: string; next_position: number }；ETag 命中返回 304 |
| GET | /api/v1/books/:book_id/jump_to_chapter?chapter_id=number | Auth | - | { position: number } |
| PUT | /api/v1/books/:book_id | Auth | Body: { title?: string; author?: string; is_public?: boolean } | {} |
| DELETE | /api/v1/books/:book_id | Auth | - | {} |
| GET | /api/v1/books/public?page=number&limit=number | Auth | - | { total: number; books: PublicBook[] } |
| GET | /api/v1/books/:book_id/tags | Auth | - | { tags: string[] } |
| PUT | /api/v1/books/:book_id/tags | Auth（仅 owner） | Body: { tags: string[] }（覆盖式） | { tags: string[] } |

### 书签 / 批注

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| GET | /api/v1/books/:book_id/bookmarks | Auth | - | { bookmarks: Bookmark[] } |
| POST | /api/v1/books/:book_id/bookmarks | Auth | Body: { position: number; label?: string }; 可选请求头 `X-Device-Id` 用于排除 ws 回环 | Bookmark |
| DELETE | /api/v1/books/:book_id/bookmarks/:bookmark_id | Auth | 可选请求头 `X-Device-Id` | {} |
| GET | /api/v1/books/:book_id/annotations | Auth | - | { annotations: Annotation[] } |
| POST | /api/v1/books/:book_id/annotations | Auth | Body: { kind: 'note' \| 'highlight'; start_position: number; end_position?: number; excerpt?: string; content?: string }; 可选请求头 `X-Device-Id` | Annotation |
| PUT | /api/v1/books/:book_id/annotations/:annotation_id | Auth | Body: { excerpt?: string; content?: string }; 可选请求头 `X-Device-Id` | {} |
| DELETE | /api/v1/books/:book_id/annotations/:annotation_id | Auth | 可选请求头 `X-Device-Id` | {} |

### 阅读进度与设置

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| GET | /api/v1/reading/settings | Auth | Header: If-None-Match? | 200 + ETag：ReadingSettings；ETag 命中返回 304 |
| PUT | /api/v1/reading/settings | Auth | Body: 部分字段（font_size? / background_color? / text_color? / line_height? / letter_spacing? / paragraph_spacing? / reading_width? / text_indent? / simplified_chinese? / timezone? / font_family? / theme? / pagination_mode?） | {} |
| GET | /api/v1/reading/stats | Auth | - | { total_reading_seconds: number; current_streak_days: number; longest_streak_days: number; recent_days: { date: string; reading_seconds: number }[] } |

### 推荐

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| GET | /api/v1/books/random_public?count=number（默认1） | Auth | - | { books: PublicBook[] } |

### 阅读会话 WebSocket

`GET /api/v1/reading/ws` 升级为 WebSocket，**替代** 旧的每秒轮询 `POST /heartbeat`。
鉴权：浏览器原生 WebSocket 无法设置请求头，因此通过 query 参数 `?token=<jwt>` 传递；非浏览器客户端也可继续用 `Authorization: Bearer <jwt>` 头。

#### 客户端 → 服务端（JSON 文本帧，`type` 取值如下）

| type | 字段 | 说明 |
|---|---|---|
| `subscribe` | `book_id: number`, `device_id: string` | 进入阅读页面时调用，声明本连接的设备标识并开始接收该书进度广播；同一连接再次 subscribe 同一本书会以新 `device_id` 覆盖旧订阅 |
| `heartbeat` | `book_id: number`, `position: number` | 上报阅读位置；服务端据此累加阅读时间并广播进度。必须先 subscribe 才能心跳 |
| `unsubscribe` | `book_id: number` | 离开阅读页面时调用 |

#### 服务端 → 客户端（JSON 文本帧，`type` 取值如下）

| type | 字段 | 说明 |
|---|---|---|
| `subscribed` | `book_id` | 订阅成功 |
| `heartbeat_ack` | `book_id`, `synced: boolean`, `position: number`, `reading_time: number` | 心跳响应。`synced=false` 表示检测到其他设备正在阅读，客户端应跳转到返回的 `position` |
| `progress_update` | `book_id`, `position: number`, `reading_time: number`, `device_id: string` | 广播：同账号下另一台设备刚刚更新了进度，本设备应跳转以保持多端同步 |
| `bookmark_changed` | `book_id`, `action: 'created' \| 'deleted'`, `bookmark_id: number` | 广播：另一端刚增删了书签；客户端收到后应重新拉取书签列表 |
| `annotation_changed` | `book_id`, `action: 'created' \| 'updated' \| 'deleted'`, `annotation_id: number` | 广播：另一端刚增改删了批注；客户端收到后应重新拉取批注列表 |
| `unsubscribed` | `book_id` | 退订成功 |
| `error` | `message: string` | 业务/协议错误；连接不会关闭 |

#### 行为细节

- 服务端每 30s 主动发送 WebSocket `Ping`；客户端无响应超过 120s 将被断开。
- 心跳间隔需 ≤ 30s，否则相邻两次心跳的间隔不计入阅读时间。
- 阅读时长 / 连续阅读天数按用户的 `reading_settings.timezone` 归算（默认 UTC），避免跨时区用户在日期边界错位。
- 当某次心跳与 `last_device_id` 不一致时（用户在另一台设备阅读中），返回服务端最新进度并 `synced=false`，**不广播**。
- 书签 / 批注的 HTTP 写接口会向订阅同 (user, book) 的其它 ws 连接广播 `bookmark_changed` / `annotation_changed`；请求头 `X-Device-Id` 可用于让服务端排除发起方连接，避免回环通知。
