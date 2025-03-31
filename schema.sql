-- 用户表
CREATE TABLE
  users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (
      strftime (
        '%Y-%m-%dT%H:%M:%S.' || substr (strftime ('%f'), 4, 6) || 'Z'
      )
    ),
    total_reading_time INTEGER NOT NULL DEFAULT 0
  );

-- 管理员表
CREATE TABLE
  admin (
    id INTEGER PRIMARY KEY,
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (
      strftime (
        '%Y-%m-%dT%H:%M:%S.' || substr (strftime ('%f'), 4, 6) || 'Z'
      )
    )
  );

-- 系统设置表
CREATE TABLE
  settings (
    id INTEGER PRIMARY KEY,
    invite_code_required BOOLEAN NOT NULL DEFAULT 1
  );

-- 邀请码表
CREATE TABLE
  invite_codes (
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
  books (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    author TEXT,
    file_path TEXT NOT NULL,
    is_public BOOLEAN NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (
      strftime (
        '%Y-%m-%dT%H:%M:%S.' || substr (strftime ('%f'), 4, 6) || 'Z'
      )
    ),
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
  );

-- 章节表
CREATE TABLE
  chapters (
    id INTEGER PRIMARY KEY,
    book_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    position INTEGER NOT NULL,
    FOREIGN KEY (book_id) REFERENCES books (id) ON DELETE CASCADE
  );

-- 阅读进度表
CREATE TABLE
  reading_progress (
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

-- 阅读设置表
CREATE TABLE
  reading_settings (
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
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE
  );