use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::header,
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
        Admin, AdminAuthResponse, AdminLoginRequest, AdminSetupRequest, Annotation, AuthResponse,
        Book, Bookmark, ChangePasswordRequest, CreateUserRequest, DeleteAccountRequest,
        LoginRequest, User, UserInfoResponse,
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
        .route("/account", delete(delete_account))
        .route("/export", get(export_data))
        .route("/admin/setup", post(admin_setup))
        .route("/admin/login", post(admin_login))
}

// 校验用户名：长度 2-20（按字符计），且只允许中英文/字母/数字/下划线/连字符。
// 必须拒绝空白与控制字符：用户名会进入导出响应的 Content-Disposition 头等位置。
pub(crate) fn validate_username(username: &str) -> Result<(), AppError> {
    let len = username.chars().count();
    if !(2..=20).contains(&len) {
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

// RFC 5987：把 attr-char 之外的字节做 UTF-8 percent 编码，
// 用于 Content-Disposition 的 filename* 参数（支持中文文件名，且不会产生非法 header）。
fn percent_encode_filename(filename: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(filename.len());
    for byte in filename.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'!'
            | b'#'
            | b'$'
            | b'&'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~' => out.push(char::from(byte)),
            _ => write!(out, "%{byte:02X}").expect("writing to a String never fails"),
        }
    }
    out
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

    // 哈希密码（耗时操作，在开启事务前完成）
    let password_hash = hash_password(&req.password)?;

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
    if !verify_password(&req.old_password, &user.password_hash)? {
        return Err(AppError::IncorrectOldPassword);
    }

    // 哈希新密码
    let new_password_hash = hash_password(&req.new_password)?;

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

    if !verify_password(&req.password, &user.password_hash)? {
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

// 导出当前用户的数据：所有书（原 txt 文件）+ JSON 元数据（书 / 章节 / 进度 /
// 设置 / 书签 / 批注）。返回 application/zip 流，文件名包含用户名与时间戳。
async fn export_data(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
) -> Result<Response, AppError> {
    use std::io::Write;

    let user: User = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(auth.user_id)
        .fetch_one(&pool)
        .await?;

    // 拉取所有结构化数据
    let books: Vec<Book> = sqlx::query_as::<_, Book>("SELECT * FROM books WHERE user_id = ?")
        .bind(auth.user_id)
        .fetch_all(&pool)
        .await?;
    let book_ids: Vec<i64> = books.iter().map(|b| b.id).collect();

    let settings: Vec<crate::models::ReadingSettings> =
        sqlx::query_as::<_, crate::models::ReadingSettings>(
            "SELECT * FROM reading_settings WHERE user_id = ?",
        )
        .bind(auth.user_id)
        .fetch_all(&pool)
        .await?;

    // 章节按书分组
    let chapters: Vec<crate::models::ChapterResponse> = if book_ids.is_empty() {
        Vec::new()
    } else {
        // 用动态 IN 查询
        let mut qb = sqlx::QueryBuilder::new(
            "SELECT id AS chapter_id, book_id, title, position FROM chapters WHERE book_id IN (",
        );
        let mut sep = qb.separated(", ");
        for id in &book_ids {
            sep.push_bind(id);
        }
        qb.push(")");
        qb.build_query_as::<crate::models::ChapterResponse>()
            .fetch_all(&pool)
            .await?
    };

    // 书签
    let bookmarks: Vec<Bookmark> =
        sqlx::query_as::<_, Bookmark>("SELECT * FROM bookmarks WHERE user_id = ?")
            .bind(auth.user_id)
            .fetch_all(&pool)
            .await?;

    // 批注
    let annotations: Vec<Annotation> =
        sqlx::query_as::<_, Annotation>("SELECT * FROM annotations WHERE user_id = ?")
            .bind(auth.user_id)
            .fetch_all(&pool)
            .await?;

    // 进度
    let progress_rows = sqlx::query!(
        "SELECT book_id, locator, percent, reading_time, last_read_at FROM reading_progress WHERE \
         user_id = ?",
        auth.user_id
    )
    .fetch_all(&pool)
    .await?;
    let progress_json: Vec<serde_json::Value> = progress_rows
        .into_iter()
        .map(|p| {
            serde_json::json!({
                "book_id": p.book_id,
                "locator": p.locator,
                "percent": p.percent,
                "reading_time": p.reading_time,
                "last_read_at": p.last_read_at,
            })
        })
        .collect();

    // 元数据 JSON
    let metadata = serde_json::json!({
        "user": {
            "id": user.id,
            "username": user.username,
            "created_at": user.created_at,
            "total_reading_time": user.total_reading_time,
        },
        "books": books,
        "chapters": chapters,
        "reading_progress": progress_json,
        "reading_settings": settings,
        "bookmarks": bookmarks,
        "annotations": annotations,
        "exported_at": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    });
    let metadata_bytes =
        serde_json::to_vec_pretty(&metadata).map_err(|e| AppError::Internal(e.to_string()))?;

    // 在内存里构建 zip：用户的书通常不多，单用户全量导出可控
    let mut zip_buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("metadata.json", opts)
            .map_err(|e| AppError::Internal(format!("zip error: {e}")))?;
        zip.write_all(&metadata_bytes)
            .map_err(|e| AppError::Internal(format!("zip error: {e}")))?;

        // 把每本书的源文件加入 zip；文件名加序号前缀防止重名
        for (idx, b) in books.iter().enumerate() {
            let path = std::path::Path::new(&b.file_path);
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("txt");
            // 文件名做基础净化（去掉路径分隔符）
            let safe_title: String = b
                .title
                .chars()
                .map(|c| match c {
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                    other => other,
                })
                .collect();
            let entry = format!("{idx:03}_{safe_title}.{ext}");
            zip.start_file(entry, opts)
                .map_err(|e| AppError::Internal(format!("zip error: {e}")))?;
            match tokio::fs::read(&b.file_path).await {
                Ok(bytes) => {
                    zip.write_all(&bytes)
                        .map_err(|e| AppError::Internal(format!("zip error: {e}")))?;
                },
                Err(e) => {
                    tracing::warn!(error = %e, file_path = %b.file_path, "skip book file in export");
                    // 写入一个 README 说明
                    let note = format!("# 文件读取失败：{e}\n原路径：{}\n", b.file_path);
                    zip.write_all(note.as_bytes())
                        .map_err(|e| AppError::Internal(format!("zip error: {e}")))?;
                },
            }
        }

        zip.finish()
            .map_err(|e| AppError::Internal(format!("zip finalize: {e}")))?;
    }

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let filename = format!("{}_{timestamp}.zip", user.username);
    // RFC 5987 filename*：支持中文用户名，且避免控制字符导致的 header 注入
    let content_disposition = format!(
        "attachment; filename*=UTF-8''{}",
        percent_encode_filename(&filename)
    );

    Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .header(header::CONTENT_DISPOSITION, content_disposition)
        .body(Body::from(zip_buf))
        .map_err(|e| AppError::Internal(format!("failed to build export response: {e}")))
}

// 管理员首次设置
async fn admin_setup(
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    Json(req): Json<AdminSetupRequest>,
) -> Result<Json<ApiResponse<AdminAuthResponse>>, AppError> {
    // 验证用户名格式（长度 + 字符白名单）
    validate_username(&req.username)?;

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
        .await?
        .ok_or_else(|| AppError::InvalidCredentials)?;

    // 验证密码
    if !verify_password(&req.password, &admin.password_hash)? {
        return Err(AppError::IncorrectAdminPassword);
    }

    // 生成JWT令牌
    let claims = Claims::new_admin(admin.id, admin.token_version, &config);
    let token = create_token(&claims, &config)?;

    // 返回管理员令牌
    Ok(Json(ApiResponse::success(AdminAuthResponse {
        admin_token: token,
    })))
}
