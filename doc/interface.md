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
| 2001 | 书籍不存在 |
| 2002 | 用户无权限操作此书籍 |
| 2003 | 书籍文件格式不支持 |
| 2004 | 书籍文件过大 |
| 2005 | 章节不存在 |
| 3001 | 用户不存在 |
| 9999 | 服务器内部错误 |

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
type UserItem = { user_id: number; username: string; created_at: string; book_count: number; total_reading_time: number }
type AdminItem = { admin_id: number; username: string; created_at: string }
type AuditLogItem = { id: number; admin_id: number; admin_username: string; action: string; target_type: string | null; target_id: number | null; detail: string | null; created_at: string }
```

`Auth`  = 请求头 `Authorization: Bearer {token}`
`Admin` = 请求头 `Authorization: Bearer {admin_token}`

# 接口列表

### 用户认证

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| POST | /api/auth/register | - | Body: { username: string; password: string; invite_code?: string } | { user_id: number; username: string; token: string } |
| POST | /api/auth/login | - | Body: { username: string; password: string } | { user_id: number; username: string; token: string } |
| GET | /api/auth/user_info | Auth | - | { user_id: number; username: string; total_reading_time: number; book_count: number } |
| POST | /api/auth/change_password | Auth | Body: { old_password: string; new_password: string } | {} |
| POST | /api/auth/admin/setup | - | Body: { username: string; password: string } | { admin_token: string } |
| POST | /api/auth/admin/login | - | Body: { username: string; password: string } | { admin_token: string } |

### 管理员

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| GET | /api/admin/check_setup | - | - | { is_setup: boolean } |
| POST | /api/admin/invite_code | Admin | Body: { limit_times: number; description?: string } | { invite_code: string; limit_times: number; description: string \| null } |
| GET | /api/admin/invite_codes | Admin | - | { invite_codes: InviteCodeItem[] } |
| GET | /api/admin/settings | Admin | - | { invite_code_required: boolean } |
| PUT | /api/admin/settings | Admin | Body: { invite_code_required: boolean } | {} |
| GET | /api/admin/users | Admin | - | { users: UserItem[] } |
| POST | /api/admin/users/:user_id/reset_password | Admin | Body: { new_password: string } | {} |
| POST | /api/admin/users/:user_id/revoke_tokens | Admin | - | {} |
| GET | /api/admin/admins | Admin | - | { admins: AdminItem[] } |
| POST | /api/admin/admins | Admin | Body: { username: string; password: string } | {} |
| GET | /api/admin/audit_logs?page=number&limit=number | Admin | - | { total: number; logs: AuditLogItem[] } |

### 书籍管理

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| POST | /api/books/upload | Auth | FormData: file (txt/epub), title: string, author?: string, is_public: boolean | { book_id: number; title: string; author: string \| null; chapters: { chapter_id: number; title: string; position: number }[] } |
| GET | /api/books?page=number&limit=number | Auth | - | { total: number; books: Book[] } |
| GET | /api/books/:book_id | Auth | - | BookDetail |
| GET | /api/books/:book_id/content?position=number&length=number | Auth | - | { content: string; next_position: number } |
| GET | /api/books/:book_id/jump_to_chapter?chapter_id=number | Auth | - | { position: number } |
| PUT | /api/books/:book_id | Auth | Body: { title?: string; author?: string; is_public?: boolean } | {} |
| DELETE | /api/books/:book_id | Auth | - | {} |
| GET | /api/books/public?page=number&limit=number | Auth | - | { total: number; books: PublicBook[] } |

### 阅读进度与设置

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| GET | /api/reading/settings | Auth | - | ReadingSettings |
| PUT | /api/reading/settings | Auth | Body: { font_size?: number; background_color?: string; text_color?: string; line_height?: number; letter_spacing?: number; paragraph_spacing?: number; reading_width?: number; text_indent?: number; simplified_chinese?: boolean } | {} |
| POST | /api/reading/heartbeat | Auth | Body: { book_id: number; position: number; device_id: string } | { synced: boolean; position: number; reading_time: number } |

### 推荐

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| GET | /api/books/random_public?count=number（默认1） | Auth | - | { books: PublicBook[] } |
