use axum::{
    Json, Router,
    extract::State,
    response::Response,
    routing::{delete, get, post},
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sqlx::{Pool, Sqlite};

use crate::{
    auth::{AuthUser, Claims, create_token},
    config::Config,
    error::{ApiResponse, AppError},
    models::{
        Admin, AdminAuthResponse, AdminLoginRequest, AdminSetupRequest, AuthResponse,
        ChangePasswordRequest, CreateUserRequest, DeleteAccountRequest, LoginRequest, User,
        UserInfoResponse,
    },
    routes::admin::log_audit_event,
    utils::{hash_password, timing_guard_verify, verify_password},
};

// 认证路由
pub fn routes() -> Router<crate::app::AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/user_info", get(user_info))
        .route("/change_password", post(change_password))
        .route("/account", delete(delete_account))
        .route("/export", get(export_data))
        .route("/admin/setup", post(admin_setup))
        .route("/admin/login", post(admin_login))
}

// 校验用户名：长度 1-20（按字符计），且只允许中英文/字母/数字/下划线/连字符。
// 必须拒绝空白与控制字符：用户名会进入导出响应的 Content-Disposition 头等位置。
pub(crate) fn validate_username(username: &str) -> Result<(), AppError> {
    let len = username.chars().count();
    if !(1..=20).contains(&len) {
        return Err(AppError::UsernameLengthInvalid);
    }
    if !username
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
    {
        return Err(AppError::UsernameCharsInvalid);
    }
    Ok(())
}

// 校验密码非空：注册 / 改密 / setup / 重置 / 新建管理员共用同一规则
pub(crate) fn validate_password(password: &str) -> Result<(), AppError> {
    if password.is_empty() {
        return Err(AppError::PasswordEmpty);
    }
    Ok(())
}

// 用户注册
async fn register(
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<ApiResponse<AuthResponse>>, AppError> {
    // 验证用户名格式（长度 + 字符白名单）
    validate_username(&req.username)?;

    // 验证密码格式
    validate_password(&req.password)?;

    // 检查系统设置（settings 行缺失时用统一缺省值）
    let invite_code_required =
        sqlx::query_scalar::<_, bool>("SELECT invite_code_required FROM settings WHERE id = 1")
            .fetch_optional(&pool)
            .await?
            .unwrap_or(crate::models::DEFAULT_INVITE_CODE_REQUIRED);

    // 如果系统需要邀请码，但用户未提供
    if invite_code_required && req.invite_code.is_none() {
        return Err(AppError::InviteCodeRequired);
    }

    // 哈希密码（耗时操作，在开启事务前完成）
    let password_hash = hash_password(&req.password).await?;

    // 注册的三步写入（核销邀请码 / 创建用户 / 默认阅读设置）放进同一事务，
    // 中途任一步失败整体回滚，不留部分状态
    let mut tx = pool.begin().await?;

    // 原子核销邀请码：在 UPDATE 的 WHERE 中判断次数上限，靠 rows_affected
    // 判断是否核销成功，消除“先校验后核销”之间的并发超额窗口
    if let Some(invite_code) = &req.invite_code {
        let redeemed = sqlx::query!(
            "UPDATE invite_codes SET used_times = used_times + 1 WHERE code = ? AND (limit_times \
             < 0 OR used_times < limit_times)",
            invite_code
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();

        if redeemed == 0 {
            // 未核销成功：区分邀请码不存在与次数已耗尽
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM invite_codes WHERE code = ?)",
            )
            .bind(invite_code)
            .fetch_one(&mut *tx)
            .await?;
            return Err(if exists {
                AppError::InviteCodeExhausted
            } else {
                AppError::InviteCodeInvalid
            });
        }
    }

    // 直接 INSERT，依赖 UNIQUE 约束兜底用户名冲突，消除“先查后插”的 TOCTOU；
    // 冲突映射为既有的用户名已存在错误（400）而不是 500
    let user_id = match sqlx::query!(
        "INSERT INTO users (username, password_hash) VALUES (?, ?)",
        req.username,
        password_hash
    )
    .execute(&mut *tx)
    .await
    {
        Ok(result) => result.last_insert_rowid(),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(AppError::UsernameAlreadyExists);
        },
        Err(e) => return Err(AppError::Database(e)),
    };

    // 为用户创建默认阅读设置
    sqlx::query!("INSERT INTO reading_settings (user_id) VALUES (?)", user_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

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

// 锁定截止时间的 RFC3339 字符串。
// 用户与管理员登录共用同一套“5 次失败锁 15 分钟”机制与状态字段。
fn lock_deadline() -> String {
    (Utc::now() + Duration::seconds(LOCK_DURATION_SECS))
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

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
        .await?;
    let Some(user) = user else {
        // 用户不存在也执行一次等价 argon2 计算，抹平响应时序差，
        // 防止借“跳过验证”的快路径枚举用户名
        timing_guard_verify(&req.password).await;
        // 审计：用户登录失败（用户名不存在）
        log_audit_event(
            &pool,
            None,
            None,
            "user_login_failed",
            None,
            None,
            Some(&req.username),
        )
        .await;
        return Err(AppError::InvalidCredentials);
    };

    // 检查账户是否被管理员封禁（先于锁定判断，避免被封用户看到锁定提示）
    if user.status == crate::models::UserStatus::Banned {
        return Err(AppError::UserBanned);
    }

    // 检查账户是否被锁定
    if let Some(ref locked_until) = user.locked_until
        && let Ok(dt) = locked_until.parse::<DateTime<Utc>>()
        && dt > Utc::now()
    {
        return Err(AppError::UserLockedOut);
    }

    // 验证密码
    if !verify_password(&req.password, &user.password_hash).await? {
        // 失败计数与锁定判定合并为单条自增语句，消除并发错密登录的漏计窗口；
        // CASE 在达到阈值时写入新的锁定截止时间，否则清空
        let attempts: Option<i64> = sqlx::query_scalar!(
            "UPDATE users SET failed_login_attempts = failed_login_attempts + 1, locked_until = \
             CASE WHEN failed_login_attempts + 1 >= ? THEN ? ELSE NULL END WHERE id = ? RETURNING \
             failed_login_attempts",
            MAX_FAILED_ATTEMPTS,
            lock_deadline(),
            user.id
        )
        .fetch_optional(&pool)
        .await?;
        if attempts.is_some_and(|n| n >= MAX_FAILED_ATTEMPTS) {
            tracing::info!(
                user_id = user.id,
                "user account locked after repeated login failures"
            );
        }
        // 审计：用户登录失败（密码错误）
        log_audit_event(
            &pool,
            None,
            None,
            "user_login_failed",
            Some("user"),
            Some(user.id),
            Some(&user.username),
        )
        .await;
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
        status: user.status,
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
    if !verify_password(&req.old_password, &user.password_hash).await? {
        return Err(AppError::IncorrectOldPassword);
    }

    // 校验新密码非空
    validate_password(&req.new_password)?;

    // 哈希新密码
    let new_password_hash = hash_password(&req.new_password).await?;

    // 更新密码并递增 token_version：使所有已签发的旧 JWT 立即失效
    sqlx::query!(
        "UPDATE users SET password_hash = ?, token_version = token_version + 1 WHERE id = ?",
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

// 用户自助注销账户：需密码确认；删除用户记录（外键级联删书 / 进度 / 批注）
// 同时删除用户上传的书籍文件
async fn delete_account(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Json(req): Json<DeleteAccountRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // 复用 change_password 的鉴权逻辑：用密码确认身份
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(auth.user_id)
        .fetch_one(&pool)
        .await?;

    if !verify_password(&req.password, &user.password_hash).await? {
        return Err(AppError::IncorrectOldPassword);
    }

    // 删除前先取出用户的所有书籍文件路径；外键级联会删 DB
    // 记录，但文件系统需手动清理
    let book_paths =
        sqlx::query_scalar::<_, String>("SELECT file_path FROM books WHERE user_id = ?")
            .bind(auth.user_id)
            .fetch_all(&pool)
            .await?;

    // 删除用户记录（books / chapters / reading_progress / bookmarks / annotations
    // / reading_settings 都有 ON DELETE CASCADE，会一并被删）
    sqlx::query!("DELETE FROM users WHERE id = ?", auth.user_id)
        .execute(&pool)
        .await?;

    // DB 删除成功后再清理文件；失败仅记日志
    for path in book_paths {
        if std::path::Path::new(&path).exists()
            && let Err(e) = tokio::fs::remove_file(&path).await
        {
            tracing::warn!(error = %e, file_path = %path, "Failed to delete book file during account deletion");
        }
    }

    Ok(Json(ApiResponse::<()>::message(
        "Account deleted successfully",
    )))
}

// 导出当前用户的数据（自助范围：不含密码哈希）：所有书（原格式文件）+
// JSON 元数据（书 / 章节 / 标签 / 进度 / 设置 / 书签 / 批注 / 每日统计）。
// 返回 application/zip 流，文件名包含用户名与时间戳
async fn export_data(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
) -> Result<Response, AppError> {
    // 重负载任务：进入全局并发排队（zip 打包全量书籍文件）
    let _permit = crate::utils::HEAVY_TASK_LIMIT
        .acquire()
        .await
        .expect("heavy task semaphore is never closed");

    let (zip_path, filename) = crate::user_data::build_user_export_zip(
        &pool,
        auth.user_id,
        crate::user_data::ExportScope::SelfService,
    )
    .await?;

    // RFC 5987 filename*：支持中文用户名，且避免控制字符导致的 header 注入
    let content_disposition = format!(
        "attachment; filename*=UTF-8''{}",
        crate::user_data::percent_encode_filename(&filename)
    );

    crate::utils::zip::stream_temp_zip(zip_path, content_disposition).await
}

// 管理员首次设置
async fn admin_setup(
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    Json(req): Json<AdminSetupRequest>,
) -> Result<Json<ApiResponse<AdminAuthResponse>>, AppError> {
    // 验证用户名格式（长度 + 字符白名单）
    validate_username(&req.username)?;

    // 验证密码非空
    validate_password(&req.password)?;

    // 哈希密码（耗时操作，在写入前完成）
    let password_hash = hash_password(&req.password).await?;

    // 单条原子语句创建首个管理员：INSERT ... WHERE NOT EXISTS 消除
    // “先查后插”的 TOCTOU——并发 setup 只有一个能插入成功
    let admin_id = match sqlx::query!(
        "INSERT INTO admin (username, password_hash)
         SELECT ?, ? WHERE NOT EXISTS(SELECT 1 FROM admin)",
        req.username,
        password_hash
    )
    .execute(&pool)
    .await
    {
        Ok(result) if result.rows_affected() > 0 => result.last_insert_rowid(),
        // rows_affected == 0：已有管理员（初始管理员只能设置一次）
        Ok(_) => return Err(AppError::AdminAlreadySetup),
        // 用户名唯一约束兜底（防御性：表非空时理论上已被上面拦截）
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(AppError::AdminUsernameAlreadyExists);
        },
        Err(e) => return Err(e.into()),
    };

    // 初始化系统设置
    sqlx::query!("INSERT OR IGNORE INTO settings (id, invite_code_required) VALUES (1, 1)")
        .execute(&pool)
        .await?;

    // 审计：初始管理员设置完成
    log_audit_event(
        &pool,
        Some(admin_id),
        Some(&req.username),
        "admin_setup",
        None,
        None,
        None,
    )
    .await;

    // 生成JWT令牌（新管理员 token_version 默认为 0）
    let claims = Claims::new_admin(admin_id, 0, &config);
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
        .await?;
    let Some(admin) = admin else {
        // 与用户登录同理：占位验证抹平时序差，防枚举管理员用户名
        timing_guard_verify(&req.password).await;
        // 审计：管理员登录失败（用户名不存在）
        log_audit_event(
            &pool,
            None,
            None,
            "admin_login_failed",
            None,
            None,
            Some(&req.username),
        )
        .await;
        return Err(AppError::InvalidCredentials);
    };

    // 检查账户是否被锁定（与普通用户登录同一套失败锁定机制）
    if let Some(ref locked_until) = admin.locked_until
        && let Ok(dt) = locked_until.parse::<DateTime<Utc>>()
        && dt > Utc::now()
    {
        return Err(AppError::UserLockedOut);
    }

    // 验证密码
    if !verify_password(&req.password, &admin.password_hash).await? {
        // 失败计数与锁定判定合并为单条自增语句（与用户登录同一机制）
        let attempts: Option<i64> = sqlx::query_scalar!(
            "UPDATE admin SET failed_login_attempts = failed_login_attempts + 1, locked_until = \
             CASE WHEN failed_login_attempts + 1 >= ? THEN ? ELSE NULL END WHERE id = ? RETURNING \
             failed_login_attempts",
            MAX_FAILED_ATTEMPTS,
            lock_deadline(),
            admin.id
        )
        .fetch_optional(&pool)
        .await?;
        if attempts.is_some_and(|n| n >= MAX_FAILED_ATTEMPTS) {
            tracing::info!(
                admin_id = admin.id,
                "admin account locked after repeated login failures"
            );
        }
        // 审计：管理员登录失败（密码错误）
        log_audit_event(
            &pool,
            Some(admin.id),
            Some(&admin.username),
            "admin_login_failed",
            None,
            None,
            None,
        )
        .await;
        return Err(AppError::IncorrectAdminPassword);
    }

    // 登录成功：重置失败计数
    if admin.failed_login_attempts > 0 || admin.locked_until.is_some() {
        sqlx::query!(
            "UPDATE admin SET failed_login_attempts = 0, locked_until = NULL WHERE id = ?",
            admin.id
        )
        .execute(&pool)
        .await?;
    }

    // 审计：管理员登录成功
    log_audit_event(
        &pool,
        Some(admin.id),
        Some(&admin.username),
        "admin_login",
        None,
        None,
        None,
    )
    .await;

    // 生成JWT令牌
    let claims = Claims::new_admin(admin.id, admin.token_version, &config);
    let token = create_token(&claims, &config)?;

    // 返回管理员令牌
    Ok(Json(ApiResponse::success(AdminAuthResponse {
        admin_token: token,
    })))
}
