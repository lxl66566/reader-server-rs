# 错误码

| code | message |
|---|---|
| 0 | 成功 |
| 1001 | 用户名或密码错误 |
| 1002 | 邀请码无效或已用完 |
| 1003 | 用户名已存在 |
| 1004 | 未登录或登录已过期 |
| 1005 | 管理员认证失败 |
| 1006 | 管理员已设置，无法重复设置 |
| 1007 | 旧密码不正确 |
| 1008 | 账户已被锁定 |
| 1009 | 账户已被封禁 |
| 1010 | 书签不存在 |
| 1011 | 批注不存在 |
| 1012 | 批注范围无效（高亮需 end_position > start_position） |
| 1013 | 搜索关键词不能为空 |
| 1014 | 搜索 scope 参数无效（mine/public/all） |
| 2001 | 书籍不存在 |
| 2002 | 用户无权限操作此书籍 |
| 2003 | 书籍文件格式不支持 |
| 2004 | 书籍文件过大 |
| 2005 | 章节不存在 |
| 3001 | 用户不存在 |
| 9999 | 服务器内部错误 |

> 所有业务接口前缀统一为 `/api/v1`；健康检查 `/health` 不在版本前缀下。

# 通用类型与认证

```
type ApiResponse<T> = { code: number; message: string; data: T }

type ReadingSettings = {
  font_size: number; background_color: string; text_color: string;
  line_height: number; letter_spacing: number; paragraph_spacing: number;
  reading_width: number; text_indent: number; simplified_chinese: boolean
}

type Book = {
  book_id: number; title: string; author: string | null; is_public: boolean;
  created_at: string; last_read_at: string | null; position: number; reading_time: number
}
type BookDetail = Book & { chapters: { chapter_id: number; title: string; position: number }[] }
type PublicBook = { book_id: number; title: string; author: string | null; owner_username: string; created_at: string }
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

### 书签 / 批注

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| GET | /api/v1/books/:book_id/bookmarks | Auth | - | { bookmarks: Bookmark[] } |
| POST | /api/v1/books/:book_id/bookmarks | Auth | Body: { position: number; label?: string } | Bookmark |
| DELETE | /api/v1/books/:book_id/bookmarks/:bookmark_id | Auth | - | {} |
| GET | /api/v1/books/:book_id/annotations | Auth | - | { annotations: Annotation[] } |
| POST | /api/v1/books/:book_id/annotations | Auth | Body: { kind: 'note' \| 'highlight'; start_position: number; end_position?: number; excerpt?: string; content?: string } | Annotation |
| PUT | /api/v1/books/:book_id/annotations/:annotation_id | Auth | Body: { excerpt?: string; content?: string } | {} |
| DELETE | /api/v1/books/:book_id/annotations/:annotation_id | Auth | - | {} |

### 阅读进度与设置

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| GET | /api/v1/reading/settings | Auth | Header: If-None-Match? | 200 + ETag：ReadingSettings；ETag 命中返回 304 |
| PUT | /api/v1/reading/settings | Auth | Body: { font_size?: number; background_color?: string; text_color?: string; line_height?: number; letter_spacing?: number; paragraph_spacing?: number; reading_width?: number; text_indent?: number; simplified_chinese?: boolean } | {} |
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
| `unsubscribed` | `book_id` | 退订成功 |
| `error` | `message: string` | 业务/协议错误；连接不会关闭 |

#### 行为细节

- 服务端每 30s 主动发送 WebSocket `Ping`；客户端无响应超过 120s 将被断开。
- 心跳间隔需 ≤ 30s，否则相邻两次心跳的间隔不计入阅读时间。
- 当某次心跳与 `last_device_id` 不一致时（用户在另一台设备阅读中），返回服务端最新进度并 `synced=false`，**不广播**。
