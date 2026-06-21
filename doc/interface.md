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
| GET | /api/v1/books/:book_id/content?position=number&length=number | Auth | - | { content: string; next_position: number } |
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
| GET | /api/v1/reading/settings | Auth | - | ReadingSettings |
| PUT | /api/v1/reading/settings | Auth | Body: { font_size?: number; background_color?: string; text_color?: string; line_height?: number; letter_spacing?: number; paragraph_spacing?: number; reading_width?: number; text_indent?: number; simplified_chinese?: boolean } | {} |
| POST | /api/v1/reading/heartbeat | Auth | Body: { book_id: number; position: number; device_id: string } | { synced: boolean; position: number; reading_time: number } |

### 推荐

| 方法 | URL | 权限 | 请求参数 | 响应 data |
|---|---|---|---|---|
| GET | /api/v1/books/random_public?count=number（默认1） | Auth | - | { books: PublicBook[] } |
