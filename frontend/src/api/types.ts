// API types mirroring doc/interface.md. Keep in sync with the backend.

// Admin: global book list item (GET /api/v1/admin/books).
export interface AdminBookItem {
  author: null | string;
  book_id: number;
  created_at: string;
  /** 磁盘文件字节数；文件缺失（被外部清理）时为 null */
  file_size: null | number;
  format: BookFormat;
  is_public: boolean;
  owner_id: number;
  owner_username: string;
  title: string;
  total_chars: number;
  total_pages: number;
}

export interface AdminItem {
  admin_id: number;
  created_at: string;
  username: string;
}

export interface AdminUserItem {
  banned_at: null | string;
  book_count: number;
  created_at: string;
  status: 'active' | 'banned';
  total_reading_time: number;
  user_id: number;
  username: string;
}

export interface Annotation {
  book_id: number;
  content: null | string;
  created_at: string;
  end_locator: null | string;
  end_percent: null | number;
  excerpt: null | string;
  id: number;
  kind: AnnotationKind;
  start_locator: string;
  start_percent: number;
  updated_at: string;
  user_id: number;
}

export type AnnotationKind = 'highlight' | 'note';

export interface ApiResponse<T> {
  code: number;
  data: T;
  message: string;
}

export interface AuditLogItem {
  action: string;
  // 登录类事件发生在认证之前，无操作管理员，故可空
  admin_id: null | number;
  admin_username: null | string;
  created_at: string;
  detail: null | string;
  id: number;
  target_id: null | number;
  target_type: null | string;
}

export interface AuthResult {
  token: string;
  user_id: number;
  username: string;
}

export interface Book {
  author: null | string;
  book_id: number;
  created_at: string;
  description: null | string;
  format: BookFormat;
  is_public: boolean;
  last_read_at: null | string;
  locator: null | string;
  percent: number;
  reading_time: number;
  title: string;
  total_chars: number;
  total_pages: number;
}

export interface BookDetail extends Book {
  /** per-book custom chapter split regex (txt only); null = built-in rules */
  chapter_pattern: null | string;
  chapters: Chapter[];
  /** whether the current user owns this book */
  is_owner: boolean;
}

export type BookFormat = 'epub' | 'pdf' | 'txt';

// GET /api/v1/books 列表参数；成员缺省时行为同后端默认
export interface BookListQuery {
  format?: BookFormat;
  order?: SortOrder;
  sort?: BookSortKey;
}

export interface Bookmark {
  book_id: number;
  created_at: string;
  id: number;
  label: null | string;
  locator: string;
  percent: number;
  user_id: number;
}

// 书架排序键（?sort=），缺省 last_read
export type BookSortKey = 'author' | 'created' | 'last_read' | 'title';

export interface Chapter {
  chapter_id: number;
  position: number;
  title: string;
}

export type ChineseConversion = 'none' | 'simplified' | 'traditional';

// 区间内某日某书的阅读时长（/stats/daily 行）
export interface DailyBookReadingStat {
  book_id: number;
  date: string;
  reading_seconds: number;
  title: string;
}

export interface DailyReadingStats {
  days: DailyBookReadingStat[];
}

// 单本书每日阅读时长条目（/reading/stats/daily/:book_id 的 entries 元素）
export interface DailyStatEntry {
  date: string;
  reading_seconds: number;
}

export interface InviteCodeItem {
  created_at: string;
  description: null | string;
  id: number;
  invite_code: string;
  limit_times: number;
  used_times: number;
}

export type PaginationMode = 'paginate' | 'scroll';

export interface PublicBook {
  author: null | string;
  book_id: number;
  created_at: string;
  description: null | string;
  format: BookFormat;
  owner_username: string;
  /** viewer's progress, null when unread */
  percent: null | number;
  /** viewer's reading time in seconds, null when unread */
  reading_time: null | number;
  title: string;
  total_chars: number;
  total_pages: number;
}

export interface ReadingSettings {
  background_color: string;
  chinese_conversion: ChineseConversion;
  font_family: string;
  font_size: number;
  letter_spacing: number;
  line_height: number;
  pagination_mode: PaginationMode;
  paragraph_spacing: number;
  reading_width: number;
  text_color: string;
  text_indent: number;
  theme: ThemeMode;
  timezone: string;
}

export interface ReadingStats {
  current_streak_days: number;
  longest_streak_days: number;
  total_reading_seconds: number;
}

export type SearchScope = 'all' | 'mine' | 'public';

// 排序方向（?order=），缺省 desc
export type SortOrder = 'asc' | 'desc';

export type ThemeMode = 'dark' | 'light' | 'sepia' | 'system';

export interface UserInfo {
  book_count: number;
  status: 'active' | 'banned';
  total_reading_time: number;
  user_id: number;
  username: string;
}

// WebSocket protocol (client -> server)
export type WsClientMessage =
  | { book_id: number; device_id: string; type: 'subscribe' }
  | { book_id: number; locator: string; percent: number; type: 'heartbeat' }
  | { book_id: number; type: 'unsubscribe' };

// WebSocket protocol (server -> client)
export type WsServerMessage =
  | {
      action: 'created' | 'deleted';
      book_id: number;
      bookmark_id: number;
      type: 'bookmark_changed';
    }
  | {
      action: 'created' | 'deleted' | 'updated';
      annotation_id: number;
      book_id: number;
      type: 'annotation_changed';
    }
  | {
      book_id: number;
      device_id: string;
      locator: string;
      percent: number;
      reading_time: number;
      type: 'progress_update';
    }
  | {
      book_id: number;
      locator: string;
      percent: number;
      reading_time: number;
      synced: boolean;
      type: 'heartbeat_ack';
      /** 本次心跳是否真正写入服务端进度；false 时其余字段为服务端现值 */
      updated: boolean;
    }
  | { book_id: number; type: 'subscribed' }
  | { book_id: number; type: 'unsubscribed' }
  | { message: string; type: 'error' };
