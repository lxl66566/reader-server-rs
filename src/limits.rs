//! 输入长度/数量上限：WS 协议（ws::protocol）与 HTTP 写接口共用，
//! 集中定义防止两处各自漂移

/// `device_id` 最大长度（字符数，字符集限定 ASCII 故等于字节数）
pub const DEVICE_ID_MAX_CHARS: usize = 64;

/// `locator` 最大长度（字节数，4KB 足够覆盖 EpubCFI 等最长格式）
pub const LOCATOR_MAX_BYTES: usize = 4 * 1024;

/// 书签 `label` 最大长度（字节数）
pub const BOOKMARK_LABEL_MAX_BYTES: usize = 256;

/// 批注 `excerpt` / `content` 最大长度（字节数；选中文本按 UTF-8 计，
/// 8KB 约覆盖一整页 CJK 文本）
pub const ANNOTATION_TEXT_MAX_BYTES: usize = 8 * 1024;

/// 每日统计单次提交 `entries` 数量上限（覆盖多年逐日补录；
/// 同时保证批量 INSERT 的绑定参数远低于 SQLite 上限）
pub const DAILY_STATS_MAX_ENTRIES: usize = 1000;

/// 书籍简介最大长度（字符数，按 `chars().count()` 计，CJK 友好）
pub const BOOK_DESCRIPTION_MAX_CHARS: usize = 2000;
