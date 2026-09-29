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

    #[error("Username must be between 2 and 20 characters")]
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

    #[error("File encoding is not valid UTF-8")]
    FileInvalidUtf8,

    #[error("Only TXT, EPUB and PDF formats are supported")]
    UnsupportedFileFormat,

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

    #[error("Setting '{0}' is out of the allowed range")]
    SettingOutOfRange(&'static str),

    #[error("Failed to parse form data: {0}")]
    FormParseError(String),

    #[error("Failed to read field '{0}': {1}")]
    FieldReadError(String, String),

    #[error(transparent)]
    Database(#[from] sqlx::Error),

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
            | AppError::UserBanned => StatusCode::UNAUTHORIZED,

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
            | AppError::PositionExceedsRange
            | AppError::InvalidPosition
            | AppError::InvalidTimezone
            | AppError::InvalidFontFamily
            | AppError::InvalidColor
            | AppError::SettingOutOfRange(_)
            | AppError::AnnotationRangeInvalid
            | AppError::SearchQueryEmpty
            | AppError::InvalidSearchScope
            | AppError::InvalidPercent
            | AppError::EmptyLocator
            | AppError::ContentEndpointNotSupported
            | AppError::FormParseError(_)
            | AppError::FieldReadError(..) => StatusCode::BAD_REQUEST,

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
