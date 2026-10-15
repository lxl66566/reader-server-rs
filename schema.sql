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
    -- token 版本号：改密时递增，使已签发的旧 admin JWT 立即失效
    token_version INTEGER NOT NULL DEFAULT 0,
    -- 登录失败锁定（与 users 表同一套机制）：连续失败计数与锁定截止时间
    failed_login_attempts INTEGER NOT NULL DEFAULT 0,
    locked_until TEXT,
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
    -- 操作管理员；登录类等认证前事件没有管理员身份，这两列为 NULL
    admin_id INTEGER,
    admin_username TEXT,
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
-- 设计要点：
-- - 所有书籍本体（原文件）存储在文件系统，DB 只存路径与派生元数据
-- - format 决定 locator 的语义：txt=字符偏移字符串、epub=EpubCFI、pdf=页码字符串
-- - total_chars 仅 txt/epub 有意义（用于章节搜索边界）；total_pages 仅 pdf 有意义
CREATE TABLE
  IF NOT EXISTS books (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    author TEXT,
    -- 原文件存储路径（绝对或相对 book_dir）
    file_path TEXT NOT NULL,
    is_public BOOLEAN NOT NULL DEFAULT 0,
    -- 封面路径：字段保留但封面功能有意暂不实现（无上传/展示链路），见 review.md
    cover_path TEXT,
    -- 章节内容总字数（chars().count()）；txt/epub 在上传时计算，pdf 为 0
    total_chars INTEGER NOT NULL DEFAULT 0,
    -- 总页数；仅 pdf 有意义，其它格式为 0
    total_pages INTEGER NOT NULL DEFAULT 0,
    -- 简介 / 摘要
    description TEXT,
    -- 文件格式：'txt' / 'epub' / 'pdf'，决定前端渲染方式与 locator 语义
    format TEXT NOT NULL CHECK (format IN ('txt', 'epub', 'pdf')),
    -- 原始文件的 SHA-256（hex），仅作内容标识（ETag 缓存校验、问题溯源），
    -- 上传时不做跨用户去重（去重涉及文件共享与引用计数，未实现）
    content_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (
      strftime (
        '%Y-%m-%dT%H:%M:%S.' || substr (strftime ('%f'), 4, 6) || 'Z'
      )
    ),
    -- 每本书独立的章节切分正则（仅 txt 有意义）；NULL/空 = 使用内置默认章节规则
    chapter_pattern TEXT,
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
-- position 字段对 txt 是全文字符偏移；对 epub 是 spine 索引（解析后的章节顺序）；
-- pdf 通常没有章节概念（如有 outline 可后续扩展），目前不写
CREATE TABLE
  IF NOT EXISTS chapters (
    id INTEGER PRIMARY KEY,
    book_id INTEGER NOT NULL,
    title TEXT NOT NULL,
    position INTEGER NOT NULL,
    FOREIGN KEY (book_id) REFERENCES books (id) ON DELETE CASCADE
  );

-- 书签：用户在书中的位置标记
-- locator 是与 format 相关的位置字符串（与 reading_progress.locator 同语义）
-- percent 是该位置的进度（0-1），便于列表展示，由前端上报
CREATE TABLE
  IF NOT EXISTS bookmarks (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    book_id INTEGER NOT NULL,
    locator TEXT NOT NULL,
    percent REAL NOT NULL DEFAULT 0 CHECK (percent >= 0.0 AND percent <= 1.0),
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
-- start_locator / end_locator 是与 format 相关的位置字符串
-- start_percent / end_percent 用于列表/统计
CREATE TABLE
  IF NOT EXISTS annotations (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    book_id INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('note', 'highlight')),
    start_locator TEXT NOT NULL,
    end_locator TEXT,
    start_percent REAL NOT NULL DEFAULT 0 CHECK (start_percent >= 0.0 AND start_percent <= 1.0),
    end_percent REAL CHECK (end_percent IS NULL OR (end_percent >= 0.0 AND end_percent <= 1.0)),
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
-- locator：与 format 相关的位置字符串（txt=数字字符偏移、epub=EpubCFI、pdf=页码）
-- percent：阅读进度（0-1），由前端计算并上报；后端不解析 locator，仅持久化+广播+列表展示
CREATE TABLE
  IF NOT EXISTS reading_progress (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    book_id INTEGER NOT NULL,
    locator TEXT NOT NULL DEFAULT '',
    percent REAL NOT NULL DEFAULT 0.0 CHECK (percent >= 0.0 AND percent <= 1.0),
    reading_time INTEGER NOT NULL DEFAULT 0,
    last_read_at TEXT,
    last_device_id TEXT,
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE,
    FOREIGN KEY (book_id) REFERENCES books (id) ON DELETE CASCADE,
    UNIQUE (user_id, book_id)
  );

-- 每日阅读统计：用户在某本书上每天的累计阅读时长（秒）
-- 由心跳累加生成；用于连续阅读天数、每日时长堆叠图、分书籍时长等可视化
CREATE TABLE
  IF NOT EXISTS daily_reading_stats (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    book_id INTEGER NOT NULL,
    date TEXT NOT NULL,
    reading_seconds INTEGER NOT NULL DEFAULT 0,
    UNIQUE (user_id, book_id, date),
    FOREIGN KEY (user_id) REFERENCES users (id) ON DELETE CASCADE,
    FOREIGN KEY (book_id) REFERENCES books (id) ON DELETE CASCADE
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
    reading_width INTEGER NOT NULL DEFAULT 40,
    text_indent REAL NOT NULL DEFAULT 2,
    -- 简繁转换：none=不转换；simplified=转简体；traditional=转繁体
    chinese_conversion TEXT NOT NULL DEFAULT 'none' CHECK (chinese_conversion IN ('none', 'simplified', 'traditional')),
    -- 用户时区（IANA 名称，如 Asia/Shanghai），影响 daily_reading_stats 归算
    -- 与连续阅读天数判断；默认 UTC
    timezone TEXT NOT NULL DEFAULT 'UTC',
    -- 字体族标识（前端解释），允许预设或自定义字体名/URL
    font_family TEXT NOT NULL DEFAULT 'serif',
    -- 主题预设；与 background_color/text_color 解耦，方便前端做"跟随系统"等
    theme TEXT NOT NULL DEFAULT 'system' CHECK (theme IN ('light', 'sepia', 'dark', 'system')),
    -- 翻页模式：scroll=滚动；paginate=翻页
    pagination_mode TEXT NOT NULL DEFAULT 'scroll' CHECK (pagination_mode IN ('scroll', 'paginate')),
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
CREATE INDEX IF NOT EXISTS idx_daily_reading_book ON daily_reading_stats (book_id);
CREATE INDEX IF NOT EXISTS idx_book_tags_tag_id ON book_tags (tag_id);
-- 审计日志按 created_at 倒序分页
CREATE INDEX IF NOT EXISTS idx_audit_logs_created_at ON audit_logs (created_at);
