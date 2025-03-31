use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use sqlx::{Pool, Sqlite};

use crate::{
    auth::{create_token, AuthUser, Claims},
    config::Config,
    error::{ApiResponse, AppError},
    models::{
        Admin, AdminSetupRequest, ChangePasswordRequest, CreateUserRequest, LoginRequest, User,
        UserInfoResponse,
    },
    utils::{generate_invite_code, hash_password, verify_password},
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
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    // 验证用户名格式
    if req.username.len() < 2 || req.username.len() > 20 {
        return Err(AppError::Validation(
            "用户名长度必须在 2-20 个字符之间".to_string(),
        ));
    }

    // 验证密码格式
    if req.password.is_empty() {
        return Err(AppError::Validation("密码不能为空".to_string()));
    }

    // 检查系统设置
    let invite_code_required =
        sqlx::query_scalar::<_, bool>("SELECT invite_code_required FROM settings WHERE id = 1")
            .fetch_optional(&pool)
            .await?
            .unwrap_or(false);

    // 如果系统需要邀请码，但用户未提供
    if invite_code_required && req.invite_code.is_none() {
        return Err(AppError::Validation("注册需要邀请码".to_string()));
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
                return Err(AppError::Validation("邀请码已用完".to_string()));
            }
            None => {
                return Err(AppError::Validation("邀请码无效".to_string()));
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
        return Err(AppError::Validation("用户名已存在".to_string()));
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

    // 生成JWT令牌
    let claims = Claims::new_user(user_id, &config);
    let token = create_token(&claims, &config)?;

    // 返回用户信息和令牌
    Ok(Json(ApiResponse::success(serde_json::json!({
        "user_id": user_id,
        "username": req.username,
        "token": token
    }))))
}

// 用户登录
async fn login(
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    // 查找用户
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?")
        .bind(&req.username)
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| AppError::Auth("用户名或密码错误".to_string()))?;

    // 验证密码
    if !verify_password(&req.password, &user.password_hash)? {
        return Err(AppError::Auth("用户名或密码错误".to_string()));
    }

    // 生成JWT令牌
    let claims = Claims::new_user(user.id, &config);
    let token = create_token(&claims, &config)?;

    // 返回用户信息和令牌
    Ok(Json(ApiResponse::success(serde_json::json!({
        "user_id": user.id,
        "username": user.username,
        "token": token
    }))))
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
        return Err(AppError::Validation("旧密码不正确".to_string()));
    }

    // 验证新密码
    if req.new_password.len() < 6 {
        return Err(AppError::Validation(
            "新密码长度必须大于6个字符".to_string(),
        ));
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
    Ok(Json(ApiResponse::<()>::message("密码修改成功")))
}

// 管理员首次设置密码
async fn admin_setup(
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    Json(req): Json<AdminSetupRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    // 验证密码格式
    if req.password.len() < 6 {
        return Err(AppError::Validation("密码长度必须大于6个字符".to_string()));
    }

    // 检查是否已有管理员
    let admin_exists = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM admin)")
        .fetch_one(&pool)
        .await?;

    if admin_exists {
        return Err(AppError::Validation(
            "管理员已设置，无法重复设置".to_string(),
        ));
    }

    // 哈希密码
    let password_hash = hash_password(&req.password)?;

    // 创建管理员
    let admin_id = sqlx::query!(
        "INSERT INTO admin (password_hash) VALUES (?)",
        password_hash
    )
    .execute(&pool)
    .await?
    .last_insert_rowid();

    // 初始化系统设置
    sqlx::query!("INSERT OR IGNORE INTO settings (id, invite_code_required) VALUES (1, 1)")
        .execute(&pool)
        .await?;

    // 创建初始邀请码
    let invite_code = generate_invite_code();
    sqlx::query!(
        "INSERT INTO invite_codes (code, limit_times, description) VALUES (?, ?, ?)",
        invite_code,
        10,
        "初始邀请码"
    )
    .execute(&pool)
    .await?;

    // 生成JWT令牌
    let claims = Claims::new_admin(admin_id, &config);
    let token = create_token(&claims, &config)?;

    // 返回管理员令牌
    Ok(Json(ApiResponse::success(serde_json::json!({
        "admin_token": token
    }))))
}

// 管理员登录
async fn admin_login(
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    Json(req): Json<AdminSetupRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    // 查找管理员
    let admin = sqlx::query_as::<_, Admin>("SELECT * FROM admin LIMIT 1")
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| AppError::Auth("管理员未设置".to_string()))?;

    // 验证密码
    if !verify_password(&req.password, &admin.password_hash)? {
        return Err(AppError::Auth("管理员密码错误".to_string()));
    }

    // 生成JWT令牌
    let claims = Claims::new_admin(admin.id, &config);
    let token = create_token(&claims, &config)?;

    // 返回管理员令牌
    Ok(Json(ApiResponse::success(serde_json::json!({
        "admin_token": token
    }))))
}
