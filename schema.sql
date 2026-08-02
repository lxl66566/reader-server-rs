-- 用户表
CREATE TABLE
  IF NOT EXISTS users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (
      strftime (
        '%Y-%m-%dT%H:%M:%S.' || substr (strftime ('%f'), 4, 6) || 'Z'
      )
    ),
    total_reading_time INTEGER NOT NULL DEFAULT 0,
    token_version INTEGER NOT NULL DEFAULT 0,
    failed_login_attempts INTEGER NOT NULL DEFAULT 0,
    locked_until TEXT,
    -- 账户状态：active=正常；banned=被管理员封禁（拒绝登录与认证）
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'banned')),
    banned_at TEXT
  );

-- 管理员表
CREATE TABLE
  IF NOT EXISTS admin (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (
      strftime (
        '%Y-%m-%dT%H:%M:%S.' || substr (strftime ('%f'), 4, 6) || 'Z'
      )
    )
  );

-- 审计日志表
CREATE TABLE
  IF NOT EXISTS audit_logs (
    id INTEGER PRIMARY KEY,
    admin_id INTEGER NOT NULL,
    admin_username TEXT NOT NULL,
    action TEXT NOT NULL,
    target_type TEXT,
    target_id INTEGER,
    detail TEXT,
    created_at TEXT NOT NULL DEFAULT (
      strftime (
        '%Y-%m-%dT%H:%M:%S.' || substr (strftime ('%f'), 4, 6) || 'Z'
      )
    ),
    FOREIGN KEY (admin_id) REFERENCES admin (id) ON DELETE CASCADE
  );

-- 系统设置表
CREATE TABLE
  IF NOT EXISTS settings (
    id INTEGER PRIMARY KEY,
    invite_code_required BOOLEAN NOT NULL DEFAULT 0
  );

-- 邀请码表
CREATE TABLE
  IF NOT EXISTS invite_codes (
    id INTEGER PRIMARY KEY,
    code TEXT NOT NULL UNIQUE,
    limit_times INTEGER NOT NULL DEFAULT 1,
    used_times INTEGER NOT NULL DEFAULT 0,
    description TEXT,
    created_at TEXT NOT NULL DEFAULT (
      strftime (
        '%Y-%m-%dT%H:%M:%S.' || substr (strftime ('%f'), 4, 6) || 'Z'
      )
    )
  );

-- 书籍表
CREATE TABLE
  IF NOT EXISTS books (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    author TEXT,
    file_path TEXT NOT NULL,
    is_public BOOLEAN NOT NULL DEFAULT 0,
    cover_path TEXT,
    created_at TEXT NOT NULL DEFAULT (
      strftime (
        '%Y-%m-%dT%H:%M:%S.' || substr (strftime ('%f'), 4, 6) || 'Z'
      )
    ),
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
  );

-- 标签字典：全局共享，避免每用户重复存储同名标签
CREATE TABLE
  IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
  );

-- 书籍 ↔ 标签 多对多
CREATE TABLE
  IF NOT EXISTS book_tags (
    book_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (book_id, tag_id),
    FOREIGN KEY (book_id) REFERENCES books (id) ON DELETE CASCADE,
    FOREIGN KEY (tag_id) REFERENCES tags (id) ON DELETE CASCADE
  );

-- 章节表
CREATE TABLE
  IF NOT EXISTS chapters (
    id INTEGER PRIMARY KEY,
    book_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    position INTEGER NOT NULL,
    FOREIGN KEY (book_id) REFERENCES books (id) ON DELETE CASCADE
  );

-- 书签：用户在书中的位置标记（轻量，仅记录位置）
CREATE TABLE
  IF NOT EXISTS bookmarks (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    book_id INTEGER NOT NULL,
    position INTEGER NOT NULL,
    label TEXT,
    created_at TEXT NOT NULL DEFAULT (
      strftime (
        '%Y-%m-%dT%H:%M:%S.' || substr (strftime ('%f'), 4, 6) || 'Z'
      )
    ),
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE,
    FOREIGN KEY (book_id) REFERENCES books (id) ON DELETE CASCADE
  );

-- 批注：用户在书中的笔记 / 高亮，按 kind 枚举区分
-- kind 取值：'note'（纯笔记）、'highlight'（高亮，需带内容片段）
CREATE TABLE
  IF NOT EXISTS annotations (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    book_id INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('note', 'highlight')),
    start_position INTEGER NOT NULL,
    end_position INTEGER,
    excerpt TEXT,
    content TEXT,
    created_at TEXT NOT NULL DEFAULT (
      strftime (
        '%Y-%m-%dT%H:%M:%S.' || substr (strftime ('%f'), 4, 6) || 'Z'
      )
    ),
    updated_at TEXT NOT NULL DEFAULT (
      strftime (
        '%Y-%m-%dT%H:%M:%S.' || substr (strftime ('%f'), 4, 6) || 'Z'
      )
    ),
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE,
    FOREIGN KEY (book_id) REFERENCES books (id) ON DELETE CASCADE
  );

-- 阅读进度表
CREATE TABLE
  IF NOT EXISTS reading_progress (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    book_id INTEGER NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    reading_time INTEGER NOT NULL DEFAULT 0,
    last_read_at TEXT,
    last_device_id TEXT,
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE,
    FOREIGN KEY (book_id) REFERENCES books (id) ON DELETE CASCADE,
    UNIQUE (user_id, book_id)
  );

-- 每日阅读统计：用户每天的累计阅读时长（秒）
-- 由心跳累加生成；用于前端连续阅读天数、每日时长图等可视化
CREATE TABLE
  IF NOT EXISTS daily_reading_stats (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    date TEXT NOT NULL,
    reading_seconds INTEGER NOT NULL DEFAULT 0,
    UNIQUE (user_id, date),
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
  );

-- 阅读设置表
CREATE TABLE
  IF NOT EXISTS reading_settings (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL UNIQUE,
    font_size INTEGER NOT NULL DEFAULT 18,
    background_color TEXT NOT NULL DEFAULT '#F5F5DC',
    text_color TEXT NOT NULL DEFAULT '#000000',
    line_height REAL NOT NULL DEFAULT 1.5,
    letter_spacing REAL NOT NULL DEFAULT 0.05,
    paragraph_spacing REAL NOT NULL DEFAULT 1.2,
    reading_width INTEGER NOT NULL DEFAULT 800,
    text_indent REAL NOT NULL DEFAULT 2,
    simplified_chinese BOOLEAN NOT NULL DEFAULT 1,
    -- 用户时区（IANA 名称，如 Asia/Shanghai），影响 daily_reading_stats 归算
    -- 与连续阅读天数判断；默认 UTC
    timezone TEXT NOT NULL DEFAULT 'UTC',
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
  );

-- 为 user_id 创建索引
CREATE INDEX IF NOT EXISTS idx_reading_settings_user_id ON reading_settings (user_id);

-- 性能关键索引
CREATE INDEX IF NOT EXISTS idx_books_user_id ON books (user_id);
CREATE INDEX IF NOT EXISTS idx_books_is_public ON books (is_public);
CREATE INDEX IF NOT EXISTS idx_chapters_book_id ON chapters (book_id);
CREATE INDEX IF NOT EXISTS idx_reading_progress_user_book ON reading_progress (user_id, book_id);
CREATE INDEX IF NOT EXISTS idx_bookmarks_user_book ON bookmarks (user_id, book_id);
CREATE INDEX IF NOT EXISTS idx_annotations_user_book ON annotations (user_id, book_id);
CREATE INDEX IF NOT EXISTS idx_daily_reading_user_date ON daily_reading_stats (user_id, date);
CREATE INDEX IF NOT EXISTS idx_book_tags_tag_id ON book_tags (tag_id);