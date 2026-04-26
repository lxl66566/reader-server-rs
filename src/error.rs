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

    #[error("Admin not set up")]
    AdminNotSetup,

    #[error("Incorrect admin password")]
    IncorrectAdminPassword,

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

    #[error("Username must be between 2 and 20 characters")]
    UsernameLengthInvalid,

    #[error("Password cannot be empty")]
    PasswordEmpty,

    #[error("Password must be at least 6 characters")]
    PasswordTooShort,

    #[error("Incorrect old password")]
    IncorrectOldPassword,

    #[error("Invite code required for registration")]
    InviteCodeRequired,

    #[error("Invite code has been exhausted")]
    InviteCodeExhausted,

    #[error("Invalid invite code")]
    InviteCodeInvalid,

    #[error("Username already exists")]
    UsernameAlreadyExists,

    #[error("Admin already set up")]
    AdminAlreadySetup,

    #[error("Title cannot be empty")]
    TitleEmpty,

    #[error("File cannot be empty")]
    FileEmpty,

    #[error("File size must not exceed 10MB")]
    FileTooLarge,

    #[error("File encoding is not valid UTF-8")]
    FileInvalidUtf8,

    #[error("Only TXT and EPUB formats are supported")]
    UnsupportedFileFormat,

    #[error("Position exceeds content range")]
    PositionExceedsRange,

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
            | AppError::AdminNotSetup
            | AppError::IncorrectAdminPassword => StatusCode::UNAUTHORIZED,

            AppError::BookAccessDenied
            | AppError::BookModifyDenied
            | AppError::BookDeleteDenied
            | AppError::AdminPermissionRequired => StatusCode::FORBIDDEN,

            AppError::BookNotFound | AppError::UserNotFound | AppError::ChapterNotFound => {
                StatusCode::NOT_FOUND
            }

            AppError::UsernameLengthInvalid
            | AppError::PasswordEmpty
            | AppError::PasswordTooShort
            | AppError::IncorrectOldPassword
            | AppError::InviteCodeRequired
            | AppError::InviteCodeExhausted
            | AppError::InviteCodeInvalid
            | AppError::UsernameAlreadyExists
            | AppError::AdminAlreadySetup
            | AppError::TitleEmpty
            | AppError::FileEmpty
            | AppError::FileTooLarge
            | AppError::FileInvalidUtf8
            | AppError::UnsupportedFileFormat
            | AppError::PositionExceedsRange
            | AppError::FormParseError(_)
            | AppError::FieldReadError(_, _) => StatusCode::BAD_REQUEST,

            AppError::Database(_) | AppError::Jwt(_) | AppError::Io(_) | AppError::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };

        if matches!(status, StatusCode::INTERNAL_SERVER_ERROR) {
            tracing::error!("Internal server error: {:?}", self);
        }

        (
            status,
            Json(ErrorResponse {
                message: self.to_string(),
            }),
        )
            .into_response()
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
            message: "成功".to_string(),
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
