use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sqlx::{Pool, Sqlite};

use crate::{
    auth::{AuthUser, Claims, create_token},
    config::Config,
    error::{ApiResponse, AppError},
    models::{
        Admin, AdminAuthResponse, AdminLoginRequest, AdminSetupRequest, AuthResponse,
        ChangePasswordRequest, CreateUserRequest, LoginRequest, User, UserInfoResponse,
    },
    utils::{hash_password, verify_password},
};

// 认证路由
pub fn routes() -> Router<crate::app::AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/user_info", get(user_info))
        .route("/change_password", post(change_password))
        .route("/admin/setup", post(admin_setup))
        .route("/admin/login", post(admin_login))
}

// 用户注册
async fn register(
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<ApiResponse<AuthResponse>>, AppError> {
    // 验证用户名格式（按字符数计，支持中英文混排）
    let username_len = req.username.chars().count();
    if !(2..=20).contains(&username_len) {
        return Err(AppError::UsernameLengthInvalid);
    }

    // 验证密码格式
    if req.password.is_empty() {
        return Err(AppError::PasswordEmpty);
    }

    // 检查系统设置
    let invite_code_required =
        sqlx::query_scalar::<_, bool>("SELECT invite_code_required FROM settings WHERE id = 1")
            .fetch_optional(&pool)
            .await?
            .unwrap_or(false);

    // 如果系统需要邀请码，但用户未提供
    if invite_code_required && req.invite_code.is_none() {
        return Err(AppError::InviteCodeRequired);
    }

    // 验证邀请码
    if let Some(invite_code) = &req.invite_code {
        let invite = sqlx::query!(
            "SELECT id, limit_times, used_times FROM invite_codes WHERE code = ?",
            invite_code
        )
        .fetch_optional(&pool)
        .await?;

        match invite {
            Some(invite) if invite.limit_times >= 0 && invite.used_times >= invite.limit_times => {
                return Err(AppError::InviteCodeExhausted);
            }
            None => {
                return Err(AppError::InviteCodeInvalid);
            }
            _ => {
                // 邀请码有效
            }
        }
    }

    // 检查用户名是否已存在
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE username = ?)")
            .bind(&req.username)
            .fetch_one(&pool)
            .await?;

    if exists {
        return Err(AppError::UsernameAlreadyExists);
    }

    // 哈希密码
    let password_hash = hash_password(&req.password)?;

    // 创建用户
    let user_id = sqlx::query!(
        "INSERT INTO users (username, password_hash) VALUES (?, ?)",
        req.username,
        password_hash
    )
    .execute(&pool)
    .await?
    .last_insert_rowid();

    // 更新邀请码使用次数
    if let Some(invite_code) = &req.invite_code {
        sqlx::query!(
            "UPDATE invite_codes SET used_times = used_times + 1 WHERE code = ?",
            invite_code
        )
        .execute(&pool)
        .await?;
    }

    // 为用户创建默认阅读设置
    sqlx::query!("INSERT INTO reading_settings (user_id) VALUES (?)", user_id)
        .execute(&pool)
        .await?;

    // 生成JWT令牌（新用户 token_version 默认为 0）
    let claims = Claims::new_user(user_id, 0, &config);
    let token = create_token(&claims, &config)?;

    // 返回用户信息和令牌
    Ok(Json(ApiResponse::success(AuthResponse {
        user_id,
        username: req.username,
        token,
    })))
}

// 最大登录失败次数，超过后锁定账户
const MAX_FAILED_ATTEMPTS: i64 = 5;
// 锁定时长（秒）
const LOCK_DURATION_SECS: i64 = 15 * 60;

// 用户登录
async fn login(
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<ApiResponse<AuthResponse>>, AppError> {
    // 查找用户
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?")
        .bind(&req.username)
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| AppError::InvalidCredentials)?;

    // 检查账户是否被锁定
    if let Some(ref locked_until) = user.locked_until
        && let Ok(dt) = locked_until.parse::<DateTime<Utc>>()
        && dt > Utc::now()
    {
        return Err(AppError::UserLockedOut);
    }

    // 验证密码
    if !verify_password(&req.password, &user.password_hash)? {
        // 登录失败：增加失败计数，达到阈值时锁定
        let new_attempts = user.failed_login_attempts + 1;
        if new_attempts >= MAX_FAILED_ATTEMPTS {
            let locked_until = Utc::now() + Duration::seconds(LOCK_DURATION_SECS);
            let locked_until_str = locked_until.to_rfc3339_opts(SecondsFormat::Millis, true);
            sqlx::query!(
                "UPDATE users SET failed_login_attempts = ?, locked_until = ? WHERE id = ?",
                new_attempts,
                locked_until_str,
                user.id
            )
            .execute(&pool)
            .await?;
        } else {
            sqlx::query!(
                "UPDATE users SET failed_login_attempts = ? WHERE id = ?",
                new_attempts,
                user.id
            )
            .execute(&pool)
            .await?;
        }
        return Err(AppError::InvalidCredentials);
    }

    // 登录成功：重置失败计数
    if user.failed_login_attempts > 0 || user.locked_until.is_some() {
        sqlx::query!(
            "UPDATE users SET failed_login_attempts = 0, locked_until = NULL WHERE id = ?",
            user.id
        )
        .execute(&pool)
        .await?;
    }

    // 生成JWT令牌
    let claims = Claims::new_user(user.id, user.token_version, &config);
    let token = create_token(&claims, &config)?;

    // 返回用户信息和令牌
    Ok(Json(ApiResponse::success(AuthResponse {
        user_id: user.id,
        username: user.username,
        token,
    })))
}

// 获取用户信息
async fn user_info(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
) -> Result<Json<ApiResponse<UserInfoResponse>>, AppError> {
    // 查询用户信息
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(auth.user_id)
        .fetch_one(&pool)
        .await?;

    // 查询书籍数量
    let book_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM books WHERE user_id = ?")
        .bind(auth.user_id)
        .fetch_one(&pool)
        .await?;

    // 返回用户信息
    Ok(Json(ApiResponse::success(UserInfoResponse {
        user_id: user.id,
        username: user.username,
        total_reading_time: user.total_reading_time,
        book_count,
    })))
}

// 修改密码
async fn change_password(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // 查询用户信息
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(auth.user_id)
        .fetch_one(&pool)
        .await?;

    // 验证旧密码
    if !verify_password(&req.old_password, &user.password_hash)? {
        return Err(AppError::IncorrectOldPassword);
    }

    // 验证新密码
    if req.new_password.chars().count() < 6 {
        return Err(AppError::PasswordTooShort);
    }

    // 哈希新密码
    let new_password_hash = hash_password(&req.new_password)?;

    // 更新密码
    sqlx::query!(
        "UPDATE users SET password_hash = ? WHERE id = ?",
        new_password_hash,
        auth.user_id
    )
    .execute(&pool)
    .await?;

    // 返回成功信息
    Ok(Json(ApiResponse::<()>::message(
        "Password changed successfully",
    )))
}

// 管理员首次设置
async fn admin_setup(
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    Json(req): Json<AdminSetupRequest>,
) -> Result<Json<ApiResponse<AdminAuthResponse>>, AppError> {
    // 验证用户名格式
    let username_len = req.username.chars().count();
    if !(2..=20).contains(&username_len) {
        return Err(AppError::UsernameLengthInvalid);
    }

    // 验证密码格式
    if req.password.chars().count() < 6 {
        return Err(AppError::PasswordTooShort);
    }

    // 检查是否已有管理员（初始管理员只能设置一次）
    let admin_exists = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM admin)")
        .fetch_one(&pool)
        .await?;

    if admin_exists {
        return Err(AppError::AdminAlreadySetup);
    }

    // 检查用户名唯一
    let username_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM admin WHERE username = ?)")
            .bind(&req.username)
            .fetch_one(&pool)
            .await?;
    if username_exists {
        return Err(AppError::AdminUsernameAlreadyExists);
    }

    // 哈希密码
    let password_hash = hash_password(&req.password)?;

    // 创建管理员
    let admin_id = sqlx::query!(
        "INSERT INTO admin (username, password_hash) VALUES (?, ?)",
        req.username,
        password_hash
    )
    .execute(&pool)
    .await?
    .last_insert_rowid();

    // 初始化系统设置
    sqlx::query!("INSERT OR IGNORE INTO settings (id, invite_code_required) VALUES (1, 1)")
        .execute(&pool)
        .await?;

    // 生成JWT令牌
    let claims = Claims::new_admin(admin_id, &config);
    let token = create_token(&claims, &config)?;

    // 返回管理员令牌
    Ok(Json(ApiResponse::success(AdminAuthResponse {
        admin_token: token,
    })))
}

// 管理员登录
async fn admin_login(
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    Json(req): Json<AdminLoginRequest>,
) -> Result<Json<ApiResponse<AdminAuthResponse>>, AppError> {
    // 查找管理员
    let admin = sqlx::query_as::<_, Admin>("SELECT * FROM admin WHERE username = ?")
        .bind(&req.username)
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| AppError::InvalidCredentials)?;

    // 验证密码
    if !verify_password(&req.password, &admin.password_hash)? {
        return Err(AppError::IncorrectAdminPassword);
    }

    // 生成JWT令牌
    let claims = Claims::new_admin(admin.id, &config);
    let token = create_token(&claims, &config)?;

    // 返回管理员令牌
    Ok(Json(ApiResponse::success(AdminAuthResponse {
        admin_token: token,
    })))
}
