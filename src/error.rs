use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("数据库错误: {0}")]
    Database(#[from] sqlx::Error),

    #[error("身份验证错误: {0}")]
    Auth(String),

    #[error("JWT错误: {0}")]
    Jwt(#[from] jsonwebtoken::errors::Error),

    #[error("参数验证失败: {0}")]
    Validation(String),

    #[error("资源不存在: {0}")]
    NotFound(String),

    #[error("权限不足: {0}")]
    Forbidden(String),

    #[error("IO错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("内部服务器错误: {0}")]
    Internal(String),

    #[error("请求处理错误: {0}")]
    BadRequest(String),

    #[error("内容解析错误: {0}")]
    ParseError(String),
}

#[derive(Serialize, Deserialize)]
pub struct ErrorResponse {
    pub code: i32,
    pub message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_code, message) = match &self {
            AppError::Auth(_) => (StatusCode::UNAUTHORIZED, 1001, self.to_string()),
            AppError::Validation(msg) if msg.contains("邀请码") => {
                (StatusCode::BAD_REQUEST, 1002, self.to_string())
            }
            AppError::Validation(msg) if msg.contains("用户名已存在") => {
                (StatusCode::BAD_REQUEST, 1003, self.to_string())
            }
            AppError::Jwt(_) => (
                StatusCode::UNAUTHORIZED,
                1004,
                "未登录或登录已过期".to_string(),
            ),
            AppError::Forbidden(msg) if msg.contains("管理员") => {
                (StatusCode::FORBIDDEN, 1005, self.to_string())
            }
            AppError::Validation(msg) if msg.contains("管理员已设置") => {
                (StatusCode::BAD_REQUEST, 1006, self.to_string())
            }
            AppError::Validation(msg) if msg.contains("旧密码") => {
                (StatusCode::BAD_REQUEST, 1007, self.to_string())
            }
            AppError::NotFound(msg) if msg.contains("书籍") => {
                (StatusCode::NOT_FOUND, 2001, self.to_string())
            }
            AppError::Forbidden(msg) if msg.contains("书籍") => {
                (StatusCode::FORBIDDEN, 2002, self.to_string())
            }
            AppError::Validation(msg) if msg.contains("格式") => {
                (StatusCode::BAD_REQUEST, 2003, self.to_string())
            }
            AppError::Validation(msg) if msg.contains("过大") => {
                (StatusCode::BAD_REQUEST, 2004, self.to_string())
            }
            AppError::NotFound(msg) if msg.contains("用户") => {
                (StatusCode::NOT_FOUND, 3001, self.to_string())
            }
            AppError::Validation(_) => (StatusCode::BAD_REQUEST, 400, self.to_string()),
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, 404, self.to_string()),
            AppError::Forbidden(_) => (StatusCode::FORBIDDEN, 403, self.to_string()),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, 400, self.to_string()),
            _ => {
                tracing::error!("内部服务器错误: {:?}", self);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    9999,
                    "服务器内部错误".to_string(),
                )
            }
        };

        let body = Json(ErrorResponse {
            code: error_code,
            message,
        });

        (status, body).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::Internal(err.to_string())
    }
}

// 定义API统一返回格式
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
