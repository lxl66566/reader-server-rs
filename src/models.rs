use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// 用户账户状态枚举
/// active=正常；banned=被管理员封禁
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum UserStatus {
    #[default]
    Active,
    Banned,
}

/// 用户角色枚举（JWT claims.role 的取值）
/// user=普通用户；admin=管理员
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    User,
    Admin,
}

impl UserRole {
    /// 角色的字符串形式（与 JWT claims.role 序列化值一致）
    pub const fn as_str(self) -> &'static str {
        match self {
            UserRole::User => "user",
            UserRole::Admin => "admin",
        }
    }
}

// 用户模型
#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub created_at: String,
    pub total_reading_time: i64,
    #[serde(skip_serializing)]
    pub token_version: i64,
    #[serde(skip_serializing)]
    pub failed_login_attempts: i64,
    #[serde(skip_serializing)]
    pub locked_until: Option<String>,
    pub status: UserStatus,
    /// 被封禁的时间戳；active 状态下为 None。保留以便后续扩展（如解封审计）
    #[serde(skip_serializing)]
    #[allow(dead_code)]
    pub banned_at: Option<String>,
}

// 创建用户请求
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub invite_code: Option<String>,
}

// 登录请求
#[derive(Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

// 用户信息响应
#[derive(Debug, Serialize)]
pub struct UserInfoResponse {
    pub user_id: i64,
    pub username: String,
    pub total_reading_time: i64,
    pub book_count: i64,
    pub status: UserStatus,
}

// 修改密码请求
#[derive(Debug, Serialize, Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

// 自助注销账户请求（需密码确认）
#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteAccountRequest {
    pub password: String,
}

// 管理员模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Admin {
    pub id: i64,
    pub username: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub token_version: i64,
    pub failed_login_attempts: i64,
    pub locked_until: Option<String>,
    pub created_at: String,
}

// 管理员首次设置密码请求
#[derive(Debug, Serialize, Deserialize)]
pub struct AdminSetupRequest {
    pub username: String,
    pub password: String,
}

// 管理员登录请求
#[derive(Debug, Serialize, Deserialize)]
pub struct AdminLoginRequest {
    pub username: String,
    pub password: String,
}

// 创建新管理员请求
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateAdminRequest {
    pub username: String,
    pub password: String,
}

// 管理员列表项
#[derive(Debug, Serialize)]
pub struct AdminListItem {
    pub admin_id: i64,
    pub username: String,
    pub created_at: String,
}

// 审计日志模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct AuditLog {
    pub id: i64,
    // 登录类等认证前事件没有操作管理员，为 None
    pub admin_id: Option<i64>,
    pub admin_username: Option<String>,
    pub action: String,
    pub target_type: Option<String>,
    pub target_id: Option<i64>,
    pub detail: Option<String>,
    pub created_at: String,
}

// 系统设置
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Settings {
    pub id: i64,
    pub invite_code_required: bool,
}

/// settings 行缺失时的统一缺省值：公开注册语义（不强制邀请码）。
/// 注册侧与管理端 GET /settings 必须引用同一常量，避免语义分叉
pub const DEFAULT_INVITE_CODE_REQUIRED: bool = false;

// 邀请码模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct InviteCode {
    pub id: i64,
    /// 对外接口与前端统一使用 invite_code（DB 列名仍为 code）
    #[serde(rename = "invite_code")]
    pub code: String,
    pub limit_times: i64,
    pub used_times: i64,
    pub description: Option<String>,
    pub created_at: String,
}

// 创建邀请码请求
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateInviteCodeRequest {
    pub limit_times: i64,
    pub description: Option<String>,
}

// 设置更新请求
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateSettingsRequest {
    pub invite_code_required: bool,
}

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
}

/// 阅读主题预设
/// light=浅色；sepia=护眼米黄；dark=深色；system=跟随系统
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum ReadingTheme {
    Light,
    Sepia,
    Dark,
    System,
}

/// 翻页模式
/// scroll=滚动；paginate=翻页
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum PaginationMode {
    Scroll,
    Paginate,
}

/// 简繁转换方向
/// none=不转换；simplified=转简体；traditional=转繁体
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum ChineseConversion {
    None,
    Simplified,
    Traditional,
}

// 阅读设置模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ReadingSettings {
    pub id: i64,
    pub user_id: i64,
    pub font_size: i64,
    pub background_color: String,
    pub text_color: String,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub paragraph_spacing: f64,
    pub reading_width: i64,
    pub text_indent: f64,
    pub chinese_conversion: ChineseConversion,
    /// IANA 时区名（如 `Asia/Shanghai`），用于阅读统计与连续天数归算
    pub timezone: String,
    /// 字体族标识（前端解释），如 `serif`/`sans-serif`/自定义字体名
    pub font_family: String,
    pub theme: ReadingTheme,
    pub pagination_mode: PaginationMode,
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

// 更新书籍请求
#[derive(Debug, Deserialize)]
pub struct UpdateBookRequest {
    pub title: Option<String>,
    pub author: Option<String>,
    pub is_public: Option<bool>,
    pub cover_path: Option<String>,
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

// 心跳协议已迁移至 ws::protocol，旧的 HTTP 心跳包数据结构已删除

// 更新阅读设置请求
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateReadingSettingsRequest {
    pub font_size: Option<i64>,
    pub background_color: Option<String>,
    pub text_color: Option<String>,
    pub line_height: Option<f64>,
    pub letter_spacing: Option<f64>,
    pub paragraph_spacing: Option<f64>,
    pub reading_width: Option<i64>,
    pub text_indent: Option<f64>,
    pub chinese_conversion: Option<ChineseConversion>,
    /// IANA 时区名；服务端会校验有效性
    pub timezone: Option<String>,
    pub font_family: Option<String>,
    pub theme: Option<ReadingTheme>,
    pub pagination_mode: Option<PaginationMode>,
}

// 管理员用户列表项
#[derive(Debug, Serialize, FromRow)]
pub struct AdminUserListItem {
    pub user_id: i64,
    pub username: String,
    pub created_at: String,
    pub book_count: i64,
    pub total_reading_time: i64,
    pub status: UserStatus,
    pub banned_at: Option<String>,
}

// 导入用户数据包响应
#[derive(Debug, Serialize)]
pub struct ImportUserResponse {
    pub user_id: i64,
    pub username: String,
    pub book_count: i64,
}

// 重置用户密码请求
#[derive(Debug, Serialize, Deserialize)]
pub struct ResetUserPasswordRequest {
    pub new_password: String,
}

// ============ 类型化响应结构体 ============

// 用户认证响应（注册/登录）
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub user_id: i64,
    pub username: String,
    pub token: String,
}

// 管理员认证响应
#[derive(Debug, Serialize)]
pub struct AdminAuthResponse {
    pub admin_token: String,
}

// 管理员检查设置响应
#[derive(Debug, Serialize)]
pub struct CheckSetupResponse {
    pub is_setup: bool,
}

// 创建邀请码响应
#[derive(Debug, Serialize)]
pub struct CreateInviteCodeResponse {
    pub invite_code: String,
    pub limit_times: i64,
    pub description: Option<String>,
}

// 邀请码列表响应
#[derive(Debug, Serialize)]
pub struct ListInviteCodesResponse {
    pub invite_codes: Vec<InviteCode>,
}

// ============ 阅读统计 ============

// 单日阅读统计
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DailyReadingStat {
    pub date: String,
    pub reading_seconds: i64,
}

// 阅读统计响应
#[derive(Debug, Serialize)]
pub struct ReadingStatsResponse {
    /// 总阅读时长（秒），与 `users.total_reading_time` 一致
    pub total_reading_seconds: i64,
    /// 连续阅读天数（含今天或昨天，按最近一次阅读计算）
    pub current_streak_days: i64,
    /// 历史最长连续阅读天数
    pub longest_streak_days: i64,
    /// 最近 N 天（默认 30）的每日时长，按日期升序
    pub recent_days: Vec<DailyReadingStat>,
}

// 用户列表响应
#[derive(Debug, Serialize)]
pub struct ListUsersResponse {
    pub users: Vec<AdminUserListItem>,
}

// 管理员列表响应
#[derive(Debug, Serialize)]
pub struct ListAdminsResponse {
    pub admins: Vec<AdminListItem>,
}

// 审计日志列表响应
#[derive(Debug, Serialize)]
pub struct ListAuditLogsResponse {
    pub total: i64,
    pub logs: Vec<AuditLog>,
}

// 书籍列表响应
#[derive(Debug, Serialize)]
pub struct BookListResponse {
    pub total: i64,
    pub books: Vec<BookListItem>,
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

// ============ 书签 / 批注 ============

/// 批注类型枚举：note=纯笔记；highlight=高亮
/// 序列化对外用字符串（保持 API 稳定可读），DB CHECK 约束同步
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum AnnotationKind {
    Note,
    Highlight,
}

// 书签模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Bookmark {
    pub id: i64,
    pub user_id: i64,
    pub book_id: i64,
    /// 与 format 相关的位置字符串（与 `reading_progress.locator` 同语义）
    pub locator: String,
    /// 进度（0-1），用于列表展示与排序
    pub percent: f64,
    pub label: Option<String>,
    pub created_at: String,
}

// 创建书签请求
#[derive(Debug, Deserialize)]
pub struct CreateBookmarkRequest {
    pub locator: String,
    /// 进度（0-1），由前端计算
    pub percent: f64,
    pub label: Option<String>,
}

// 批注模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Annotation {
    pub id: i64,
    pub user_id: i64,
    pub book_id: i64,
    pub kind: AnnotationKind,
    /// 起始位置（locator），与 format 相关
    pub start_locator: String,
    /// 结束位置（locator），仅 highlight 必填
    pub end_locator: Option<String>,
    /// 起始进度（0-1）
    pub start_percent: f64,
    /// 结束进度（0-1），仅 highlight 有
    pub end_percent: Option<f64>,
    pub excerpt: Option<String>,
    pub content: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// 创建批注请求
#[derive(Debug, Deserialize)]
pub struct CreateAnnotationRequest {
    pub kind: AnnotationKind,
    pub start_locator: String,
    pub end_locator: Option<String>,
    /// 进度（0-1），由前端计算
    pub start_percent: f64,
    pub end_percent: Option<f64>,
    pub excerpt: Option<String>,
    pub content: Option<String>,
}

// 更新批注请求（只允许改 content/excerpt，kind/位置不可改）
#[derive(Debug, Deserialize)]
pub struct UpdateAnnotationRequest {
    pub excerpt: Option<String>,
    pub content: Option<String>,
}

// 书签列表响应
#[derive(Debug, Serialize)]
pub struct ListBookmarksResponse {
    pub bookmarks: Vec<Bookmark>,
}

// 批注列表响应
#[derive(Debug, Serialize)]
pub struct ListAnnotationsResponse {
    pub annotations: Vec<Annotation>,
}
