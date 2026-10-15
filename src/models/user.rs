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

// ============ 类型化响应结构体 ============

// 用户认证响应（注册/登录）
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub user_id: i64,
    pub username: String,
    pub token: String,
}
