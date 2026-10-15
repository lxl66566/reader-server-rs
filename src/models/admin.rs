use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::{book::BookFormat, user::UserStatus};

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

// 管理员设置任意用户书籍阅读时长请求（秒）
#[derive(Debug, Deserialize)]
pub struct AdminSetReadingTimeRequest {
    pub book_id: i64,
    pub reading_time: i64,
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

// 管理员书籍列表项（全局视角，含所有者与磁盘文件大小）
#[derive(Debug, Serialize)]
pub struct AdminBookListItem {
    pub book_id: i64,
    pub title: String,
    pub author: Option<String>,
    pub owner_id: i64,
    pub owner_username: String,
    pub is_public: bool,
    pub format: BookFormat,
    pub total_chars: i64,
    pub total_pages: i64,
    /// 磁盘文件字节数；文件缺失（被外部清理）时为 null
    pub file_size: Option<i64>,
    pub created_at: String,
    /// 移入回收站的时间（仅 include_deleted=true 时非 null）
    pub deleted_at: Option<String>,
}

// 管理员书籍列表响应
#[derive(Debug, Serialize)]
pub struct AdminBookListResponse {
    pub total: i64,
    pub books: Vec<AdminBookListItem>,
}

// 管理员编辑书籍请求（全部可选；至少提供一项才会产生更新）
#[derive(Debug, Deserialize)]
pub struct AdminUpdateBookRequest {
    pub title: Option<String>,
    pub author: Option<String>,
    pub is_public: Option<bool>,
}
