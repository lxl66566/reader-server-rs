use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// 用户模型
#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub created_at: String,
    pub total_reading_time: i64,
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
    pub device_id: String,
}

// 用户信息响应
#[derive(Debug, Serialize)]
pub struct UserInfoResponse {
    pub user_id: i64,
    pub username: String,
    pub total_reading_time: i64,
    pub book_count: i64,
}

// 修改密码请求
#[derive(Debug, Serialize, Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

// 管理员模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Admin {
    pub id: i64,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub created_at: String,
}

// 管理员设置密码请求
#[derive(Debug, Serialize, Deserialize)]
pub struct AdminSetupRequest {
    pub password: String,
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
#[derive(Debug, Deserialize)]
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

// 章节模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Chapter {
    pub id: i64,
    pub book_id: i64,
    pub title: String,
    pub position: i64,
}

// 阅读进度模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ReadingProgress {
    pub id: i64,
    pub user_id: i64,
    pub book_id: i64,
    pub position: i64,
    pub reading_time: i64,
    pub last_read_at: Option<String>,
    pub last_device_id: Option<String>,
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
#[derive(Debug, Serialize)]
pub struct AdminUserListItem {
    pub user_id: i64,
    pub username: String,
    pub created_at: String,
    pub book_count: i64,
    pub total_reading_time: i64,
}

// 重置用户密码请求
#[derive(Debug, Deserialize)]
pub struct ResetPasswordRequest {
    pub new_password: String,
}
