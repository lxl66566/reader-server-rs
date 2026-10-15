use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Authorization token not provided")]
    TokenNotProvided,

    #[error("Invalid token role")]
    InvalidTokenRole,

    #[error("Invalid user ID in token")]
    InvalidUserId,

    #[error("Admin authorization token not provided")]
    AdminTokenNotProvided,

    #[error("Invalid admin ID in token")]
    InvalidAdminId,

    #[error("Token expired or invalid")]
    TokenExpiredOrInvalid,

    #[error("Incorrect admin password")]
    IncorrectAdminPassword,

    #[error("Account temporarily locked due to too many failed login attempts")]
    UserLockedOut,

    #[error("Account has been banned")]
    UserBanned,

    // token 对应用户已被删除（自查注销 / 管理员清理）；与 banned 一致按 401 处理
    #[error("Account no longer exists")]
    AccountDeleted,

    #[error("Admin not found")]
    AdminNotFound,

    #[error("Admin username already exists")]
    AdminUsernameAlreadyExists,

    #[error("Access denied to this book")]
    BookAccessDenied,

    #[error("Cannot modify this book")]
    BookModifyDenied,

    #[error("Cannot delete this book")]
    BookDeleteDenied,

    #[error("Admin permission required")]
    AdminPermissionRequired,

    #[error("Book not found")]
    BookNotFound,

    #[error("User not found")]
    UserNotFound,

    #[error("Chapter not found")]
    ChapterNotFound,

    #[error("Bookmark not found")]
    BookmarkNotFound,

    #[error("Annotation not found")]
    AnnotationNotFound,

    #[error("Highlight annotation requires end_position > start_position")]
    AnnotationRangeInvalid,

    #[error("Search query must not be empty")]
    SearchQueryEmpty,

    #[error("Invalid search scope; must be one of: mine, public, all")]
    InvalidSearchScope,

    #[error("Invalid sort; must be one of: last_read, created, title, author")]
    InvalidBookSort,

    #[error("Invalid order; must be one of: asc, desc")]
    InvalidSortOrder,

    #[error("Invalid format filter; must be one of: txt, epub, pdf")]
    InvalidFormatFilter,

    #[error("Username must be between 1 and 20 characters")]
    UsernameLengthInvalid,

    #[error("Username may only contain letters (including CJK), digits, '_' and '-'")]
    UsernameCharsInvalid,

    #[error("Password cannot be empty")]
    PasswordEmpty,

    #[error("Incorrect old password")]
    IncorrectOldPassword,

    #[error("Invite code required for registration")]
    InviteCodeRequired,

    #[error("Invite code has been exhausted")]
    InviteCodeExhausted,

    #[error("Invalid invite code")]
    InviteCodeInvalid,

    #[error("Invite code limit must be greater than 0")]
    InviteCodeLimitInvalid,

    #[error("Username already exists")]
    UsernameAlreadyExists,

    #[error("Admin already set up")]
    AdminAlreadySetup,

    #[error("Title cannot be empty")]
    TitleEmpty,

    #[error("File cannot be empty")]
    FileEmpty,

    // 不硬编码具体数值：实际上限由 storage.max_file_size 配置决定
    #[error("File size exceeds the allowed limit")]
    FileTooLarge,

    // epub 单条目解压大小或拼接全文超上限（防御 zip 炸弹），与上传体积无关
    #[error("Epub content too large after decompression")]
    EpubContentTooLarge,

    #[error("Request body too large")]
    RequestBodyTooLarge,

    // 明确指向 GBK/GB18030：中文 txt 最常见的非 UTF-8 编码
    #[error(
        "Only UTF-8 encoded text files are supported; detected non-UTF-8 bytes (possibly \
         GBK/GB18030). Please convert the file to UTF-8 and re-upload"
    )]
    FileInvalidUtf8,

    #[error("Only TXT, EPUB and PDF formats are supported")]
    UnsupportedFileFormat,

    // 上传文件内容损坏导致解析失败（区别于内部错误：这是客户端问题，400）
    #[error("Failed to parse uploaded file: {0}")]
    FileParseError(String),

    // 章节数超上限（构造性 txt 可产出百万级行），拒绝而非静默截断
    #[error("Too many chapters detected; the file appears malformed")]
    TooManyChapters,

    // 自定义章节切分正则无法编译（fancy-regex 语法错误）
    #[error("Invalid chapter regex: {0}")]
    InvalidChapterRegex(String),

    // 章节切分正则仅对 txt 有意义
    #[error("chapter_pattern is only supported for txt books")]
    ChapterPatternNotSupported,

    #[error("Position exceeds content range")]
    PositionExceedsRange,

    #[error("Position must not be negative")]
    #[allow(dead_code)]
    InvalidPosition,

    #[error("Percent must be in [0.0, 1.0]")]
    InvalidPercent,

    #[error("Locator must not be empty")]
    EmptyLocator,

    #[error("Content endpoint is only supported for txt books")]
    ContentEndpointNotSupported,

    #[error("Invalid timezone identifier (must be IANA name, e.g. Asia/Shanghai)")]
    InvalidTimezone,

    #[error("Invalid font_family (must be 1-128 chars)")]
    InvalidFontFamily,

    #[error("Invalid color value (expected hex format, e.g. #RRGGBB)")]
    InvalidColor,

    // 手动设置阅读时长越界；上限见 routes::reading::MAX_READING_TIME_SECS（3650 天）
    #[error("Reading time must be between 0 and 315360000 seconds (3650 days)")]
    ReadingTimeOutOfRange,

    #[error("Setting '{0}' is out of the allowed range")]
    SettingOutOfRange(&'static str),

    #[error(
        "Invalid date range (start must be a valid YYYY-MM-DD not after end, spanning at most 372 \
         days)"
    )]
    InvalidDateRange,

    // 每日阅读时长条目日期非法（非 YYYY-MM-DD）
    #[error("Invalid daily stats entry date (expected YYYY-MM-DD)")]
    InvalidDailyStatDate,

    // 每日阅读时长条目日期重复（会撞 UNIQUE 约束，须在入库前拒绝）
    #[error("Duplicate dates in daily stats entries")]
    DuplicateDailyStatDate,

    #[error("Failed to parse form data: {0}")]
    FormParseError(String),

    // 请求字段超过共享长度上限（具体数值见 crate::limits）
    #[error("Field '{0}' exceeds the allowed length limit")]
    FieldTooLarge(&'static str),

    // 每日统计条目数量超上限（crate::limits::DAILY_STATS_MAX_ENTRIES）
    #[error("Too many daily stats entries")]
    TooManyDailyStatsEntries,

    // 用户数据导入包无效：zip 损坏 / 缺 metadata.json / 条目缺失 / 版本不支持 / 字段非法
    #[error("Invalid user data package: {0}")]
    ImportPackageInvalid(String),

    #[error("Failed to read field '{0}': {1}")]
    FieldReadError(String, String),

    #[error(transparent)]
    Database(#[from] sqlx::Error),

    // 仅用于 token 签发（create_token）失败：内部错误，500。
    // 验证路径的 JWT 错误已在 verify_token 统一映射为 TokenExpiredOrInvalid（401）
    #[error(transparent)]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("Internal server error: {0}")]
    Internal(String),
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::InvalidCredentials
            | AppError::TokenNotProvided
            | AppError::InvalidTokenRole
            | AppError::InvalidUserId
            | AppError::AdminTokenNotProvided
            | AppError::InvalidAdminId
            | AppError::TokenExpiredOrInvalid
            | AppError::IncorrectAdminPassword
            | AppError::UserLockedOut
            | AppError::UserBanned
            | AppError::AccountDeleted => StatusCode::UNAUTHORIZED,

            AppError::BookAccessDenied
            | AppError::BookModifyDenied
            | AppError::BookDeleteDenied
            | AppError::AdminPermissionRequired => StatusCode::FORBIDDEN,

            AppError::RequestBodyTooLarge => StatusCode::PAYLOAD_TOO_LARGE,

            AppError::BookNotFound
            | AppError::UserNotFound
            | AppError::ChapterNotFound
            | AppError::BookmarkNotFound
            | AppError::AnnotationNotFound
            | AppError::AdminNotFound => StatusCode::NOT_FOUND,

            AppError::UsernameLengthInvalid
            | AppError::UsernameCharsInvalid
            | AppError::PasswordEmpty
            | AppError::IncorrectOldPassword
            | AppError::InviteCodeRequired
            | AppError::InviteCodeExhausted
            | AppError::InviteCodeInvalid
            | AppError::InviteCodeLimitInvalid
            | AppError::UsernameAlreadyExists
            | AppError::AdminUsernameAlreadyExists
            | AppError::AdminAlreadySetup
            | AppError::TitleEmpty
            | AppError::FileEmpty
            | AppError::FileTooLarge
            | AppError::EpubContentTooLarge
            | AppError::FileInvalidUtf8
            | AppError::UnsupportedFileFormat
            | AppError::FileParseError(_)
            | AppError::TooManyChapters
            | AppError::InvalidChapterRegex(_)
            | AppError::ChapterPatternNotSupported
            | AppError::PositionExceedsRange
            | AppError::InvalidPosition
            | AppError::InvalidTimezone
            | AppError::InvalidFontFamily
            | AppError::InvalidColor
            | AppError::ReadingTimeOutOfRange
            | AppError::SettingOutOfRange(_)
            | AppError::AnnotationRangeInvalid
            | AppError::SearchQueryEmpty
            | AppError::InvalidSearchScope
            | AppError::InvalidPercent
            | AppError::InvalidDateRange
            | AppError::InvalidDailyStatDate
            | AppError::DuplicateDailyStatDate
            | AppError::EmptyLocator
            | AppError::ContentEndpointNotSupported
            | AppError::FormParseError(_)
            | AppError::FieldTooLarge(_)
            | AppError::TooManyDailyStatsEntries
            | AppError::FieldReadError(..)
            | AppError::ImportPackageInvalid(_)
            | AppError::InvalidBookSort
            | AppError::InvalidSortOrder
            | AppError::InvalidFormatFilter => StatusCode::BAD_REQUEST,

            AppError::Database(_) | AppError::Jwt(_) | AppError::Io(_) | AppError::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            },
        };

        // 对内部错误（数据库/JWT/IO/Internal）记录详细日志，但只向客户端返回固定文案，
        // 避免泄露 SQL 错误、文件路径等敏感信息。
        let message = if let AppError::Database(_)
        | AppError::Jwt(_)
        | AppError::Io(_)
        | AppError::Internal(_) = &self
        {
            tracing::error!(error = ?self, "Internal server error");
            "Internal server error".to_string()
        } else {
            self.to_string()
        };

        (status, Json(ErrorResponse { message })).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::Internal(err.to_string())
    }
}

#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            code: 0,
            message: "success".to_string(),
            data: Some(data),
        }
    }

    pub fn message(message: &str) -> ApiResponse<()> {
        ApiResponse {
            code: 0,
            message: message.to_string(),
            data: None,
        }
    }
}
