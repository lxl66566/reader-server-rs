use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// 书籍文件格式枚举
///
/// 决定前端渲染方式与 locator 语义：
/// - `Txt`：纯文本，locator = 字符偏移字符串（数字）
/// - `Epub`：EPUB，locator = `EpubCFI` 字符串
/// - `Pdf`：PDF，locator = 页码字符串（"page:N"，从 0 起）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum BookFormat {
    Txt,
    Epub,
    Pdf,
}

impl BookFormat {
    /// 文件扩展名（不含点）
    pub fn ext(self) -> &'static str {
        match self {
            BookFormat::Txt => "txt",
            BookFormat::Epub => "epub",
            BookFormat::Pdf => "pdf",
        }
    }

    /// 主要的 MIME 类型
    pub fn mime_type(self) -> &'static str {
        match self {
            BookFormat::Txt => "text/plain; charset=utf-8",
            BookFormat::Epub => "application/epub+zip",
            BookFormat::Pdf => "application/pdf",
        }
    }
}

// 书籍模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Book {
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub author: Option<String>,
    pub file_path: String,
    pub is_public: bool,
    pub cover_path: Option<String>,
    pub total_chars: i64,
    pub total_pages: i64,
    pub description: Option<String>,
    pub format: BookFormat,
    pub content_hash: String,
    pub created_at: String,
    /// 本书自定义章节切分正则（仅 txt）；None = 内置默认规则
    pub chapter_pattern: Option<String>,
}

// 上传书籍响应
#[derive(Debug, Serialize)]
pub struct UploadBookResponse {
    pub book_id: i64,
    pub title: String,
    pub author: Option<String>,
    pub format: BookFormat,
    pub total_chars: i64,
    pub total_pages: i64,
    pub chapters: Vec<ChapterResponse>,
}

// 章节响应
// position 对 txt 是全文字符偏移；对 epub 是 spine 索引（0-based）
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ChapterResponse {
    pub chapter_id: i64,
    pub title: String,
    pub position: i64,
}

// 书籍列表项响应
#[derive(Debug, Serialize)]
pub struct BookListItem {
    pub book_id: i64,
    pub title: String,
    pub author: Option<String>,
    pub is_public: bool,
    pub format: BookFormat,
    pub created_at: String,
    pub last_read_at: Option<String>,
    pub locator: Option<String>,
    pub percent: f64,
    pub reading_time: i64,
    /// 全文字符数（txt/epub）；pdf 为 0
    pub total_chars: i64,
    /// 总页数（pdf）；其它格式为 0
    pub total_pages: i64,
    pub description: Option<String>,
}

// 书籍详情响应
#[derive(Debug, Serialize)]
pub struct BookDetailResponse {
    pub book_id: i64,
    pub title: String,
    pub author: Option<String>,
    pub is_public: bool,
    pub format: BookFormat,
    pub created_at: String,
    pub last_read_at: Option<String>,
    pub locator: Option<String>,
    pub percent: f64,
    pub reading_time: i64,
    pub total_chars: i64,
    pub total_pages: i64,
    pub description: Option<String>,
    /// 当前请求用户是否为书籍所有者（前端据此显示编辑/删除入口）
    pub is_owner: bool,
    /// 本书自定义章节切分正则（仅 txt）；None = 内置默认规则
    pub chapter_pattern: Option<String>,
    pub chapters: Vec<ChapterResponse>,
}

// 公开书籍列表项
#[derive(Debug, Serialize)]
pub struct PublicBookListItem {
    pub book_id: i64,
    pub title: String,
    pub author: Option<String>,
    pub owner_username: String,
    pub format: BookFormat,
    pub created_at: String,
    pub total_chars: i64,
    pub total_pages: i64,
    pub description: Option<String>,
    /// 当前请求用户的阅读进度（未读为 None）
    pub percent: Option<f64>,
    /// 当前请求用户的阅读时长（未读为 None）
    pub reading_time: Option<i64>,
}

// txt 书籍内容响应（仅 txt 格式有意义）
#[derive(Debug, Serialize)]
pub struct BookContentResponse {
    pub content: String,
    pub next_position: i64,
}

// 更新请求中 chapter_pattern 字段的三态（serde 会把 Option 的 null 坍缩成 None，需手工还原）：
// - Unchanged：字段缺省，不修改
// - Reset：null / 纯空白串，恢复内置默认规则
// - Set：非空值，设置为本书专用的切分正则（已 trim）
#[derive(Debug, Default)]
pub enum PatternUpdate {
    #[default]
    Unchanged,
    Reset,
    Set(String),
}

impl<'de> Deserialize<'de> for PatternUpdate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match Option::<String>::deserialize(deserializer)? {
            Some(v) if !v.trim().is_empty() => PatternUpdate::Set(v.trim().to_string()),
            _ => PatternUpdate::Reset,
        })
    }
}

// 更新书籍请求
#[derive(Debug, Deserialize)]
pub struct UpdateBookRequest {
    pub title: Option<String>,
    pub author: Option<String>,
    pub is_public: Option<bool>,
    /// 封面路径：字段保留但封面功能有意暂不实现（无上传/展示链路），见 review.md
    pub cover_path: Option<String>,
    /// 简介：字段缺省 = 不修改；空/纯空白串 = 清空（NULL）；上限见
    /// [`crate::limits::BOOK_DESCRIPTION_MAX_CHARS`]
    pub description: Option<String>,
    #[serde(default)]
    pub chapter_pattern: PatternUpdate,
}

// 设置书籍标签请求
#[derive(Debug, Deserialize)]
pub struct SetBookTagsRequest {
    pub tags: Vec<String>,
}

// 标签响应
#[derive(Debug, Serialize)]
pub struct BookTagsResponse {
    pub tags: Vec<String>,
}

// 书籍列表响应
#[derive(Debug, Serialize)]
pub struct BookListResponse {
    pub total: i64,
    pub books: Vec<BookListItem>,
}

// 回收站列表项
#[derive(Debug, Serialize)]
pub struct TrashBookListItem {
    pub book_id: i64,
    pub title: String,
    pub author: Option<String>,
    pub format: BookFormat,
    /// 移入回收站的时间（RFC3339）
    pub deleted_at: String,
}

// 回收站列表响应
#[derive(Debug, Serialize)]
pub struct TrashBookListResponse {
    pub total: i64,
    pub books: Vec<TrashBookListItem>,
}

// 公开书籍列表响应
#[derive(Debug, Serialize)]
pub struct PublicBookListResponse {
    pub total: i64,
    pub books: Vec<PublicBookListItem>,
}

// 随机公开书籍响应
#[derive(Debug, Serialize)]
pub struct RandomPublicBooksResponse {
    pub books: Vec<PublicBookListItem>,
}

// 跳转章节查询参数响应
// 返回该章节的起始位置（txt=字符偏移，epub=spine 索引字符串，pdf=未实现）
#[derive(Debug, Serialize)]
pub struct JumpToChapterResponse {
    /// 章节首字符在全文字符流中的偏移（txt 用），epub 用 spine 索引
    pub position: i64,
}

// 书内全文搜索单条命中
#[derive(Debug, Serialize)]
pub struct BookTextSearchMatch {
    /// 命中所在章节在按 position 升序的章节列表中的下标（0-based）
    pub chapter_index: i64,
    pub chapter_title: String,
    /// 命中起始字符的全书偏移（txt 与章节 position、/content 的 position 同一坐标系；
    /// epub 为解析拼接全文中的偏移，仅用于 excerpt 定位，阅读端按章节粒度跳转）
    pub char_offset: i64,
    /// 命中前后各约 30 字符的纯文本上下文（含命中本身；前端按 offset 自行加粗）
    pub excerpt: String,
}

// 书内全文搜索响应
#[derive(Debug, Serialize)]
pub struct BookTextSearchResponse {
    pub matches: Vec<BookTextSearchMatch>,
    /// 命中总数超过返回上限，结果已截断
    pub truncated: bool,
}
