use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// 用户账户状态枚举
/// active=正常；banned=被管理员封禁
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum UserStatus {
    Active,
    Banned,
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
    pub admin_id: i64,
    pub admin_username: String,
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

// 邀请码模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct InviteCode {
    pub id: i64,
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

// 书籍模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Book {
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub author: Option<String>,
    pub file_path: String,
    pub is_public: bool,
    pub created_at: String,
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
    pub simplified_chinese: bool,
}

// 上传书籍响应
#[derive(Debug, Serialize)]
pub struct UploadBookResponse {
    pub book_id: i64,
    pub title: String,
    pub author: Option<String>,
    pub chapters: Vec<ChapterResponse>,
}

// 章节响应
#[derive(Debug, Serialize)]
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
    pub created_at: String,
    pub last_read_at: Option<String>,
    pub position: i64,
    pub reading_time: i64,
}

// 书籍详情响应
#[derive(Debug, Serialize)]
pub struct BookDetailResponse {
    pub book_id: i64,
    pub title: String,
    pub author: Option<String>,
    pub is_public: bool,
    pub created_at: String,
    pub last_read_at: Option<String>,
    pub position: i64,
    pub reading_time: i64,
    pub chapters: Vec<ChapterResponse>,
}

// 公开书籍列表项
#[derive(Debug, Serialize)]
pub struct PublicBookListItem {
    pub book_id: i64,
    pub title: String,
    pub author: Option<String>,
    pub owner_username: String,
    pub created_at: String,
}

// 书籍内容响应
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
}

// 心跳包请求
#[derive(Debug, Deserialize)]
pub struct HeartbeatRequest {
    pub book_id: i64,
    pub position: i64,
    pub device_id: String,
}

// 心跳包响应
#[derive(Debug, Serialize)]
pub struct HeartbeatResponse {
    pub synced: bool,
    pub position: i64,
    pub reading_time: i64,
}

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
    pub simplified_chinese: Option<bool>,
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
#[derive(Debug, Serialize)]
pub struct JumpToChapterResponse {
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
    pub position: i64,
    pub label: Option<String>,
    pub created_at: String,
}

// 创建书签请求
#[derive(Debug, Deserialize)]
pub struct CreateBookmarkRequest {
    pub position: i64,
    pub label: Option<String>,
}

// 批注模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Annotation {
    pub id: i64,
    pub user_id: i64,
    pub book_id: i64,
    pub kind: AnnotationKind,
    pub start_position: i64,
    pub end_position: Option<i64>,
    pub excerpt: Option<String>,
    pub content: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// 创建批注请求
#[derive(Debug, Deserialize)]
pub struct CreateAnnotationRequest {
    pub kind: AnnotationKind,
    pub start_position: i64,
    pub end_position: Option<i64>,
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
