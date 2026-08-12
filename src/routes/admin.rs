use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::header,
    response::Response,
    routing::{get, post},
};
use chrono::{SecondsFormat, Utc};
use sqlx::{Pool, Sqlite};

use crate::{
    auth::AuthAdmin,
    config::Config,
    error::{ApiResponse, AppError},
    models::{
        AdminListItem, AdminUserListItem, AuditLog, CheckSetupResponse, CreateAdminRequest,
        CreateInviteCodeRequest, CreateInviteCodeResponse, InviteCode, ListAdminsResponse,
        ListAuditLogsResponse, ListInviteCodesResponse, ListUsersResponse,
        ResetUserPasswordRequest, Settings, UpdateSettingsRequest,
    },
    routes::books::PaginationParams,
    utils::{generate_invite_code, hash_password},
};

// 管理员路由
pub fn routes() -> Router<crate::app::AppState> {
    Router::new()
        .route("/check_setup", get(check_setup))
        .route("/invite_code", post(create_invite_code))
        .route("/invite_codes", get(list_invite_codes))
        .route("/settings", get(get_settings).put(update_settings))
        .route("/users", get(list_users))
        .route("/users/{user_id}/reset_password", post(reset_password))
        .route("/users/{user_id}/revoke_tokens", post(revoke_user_tokens))
        .route("/users/{user_id}/ban", post(ban_user))
        .route("/users/{user_id}/unban", post(unban_user))
        .route("/admins", get(list_admins).post(create_admin))
        .route("/audit_logs", get(list_audit_logs))
        .route("/backup", get(backup_global))
}

/// 记录审计日志
async fn log_audit(
    pool: &Pool<Sqlite>,
    admin: &AuthAdmin,
    action: &str,
    target_type: Option<&str>,
    target_id: Option<i64>,
    detail: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query!(
        "INSERT INTO audit_logs (admin_id, admin_username, action, target_type, target_id, \
         detail) VALUES (?, ?, ?, ?, ?, ?)",
        admin.admin_id,
        admin.username,
        action,
        target_type,
        target_id,
        detail
    )
    .execute(pool)
    .await?;
    Ok(())
}

// 检查是否已设置管理员
async fn check_setup(
    State(pool): State<Pool<Sqlite>>,
) -> Result<Json<ApiResponse<CheckSetupResponse>>, AppError> {
    let admin_exists = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM admin)")
        .fetch_one(&pool)
        .await?;

    Ok(Json(ApiResponse::success(CheckSetupResponse {
        is_setup: admin_exists,
    })))
}

// 创建邀请码
async fn create_invite_code(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Json(req): Json<CreateInviteCodeRequest>,
) -> Result<Json<ApiResponse<CreateInviteCodeResponse>>, AppError> {
    // 验证使用次数上限
    if req.limit_times <= 0 {
        return Err(AppError::InviteCodeLimitInvalid);
    }

    let invite_code = generate_invite_code();

    sqlx::query!(
        "INSERT INTO invite_codes (code, limit_times, description) VALUES (?, ?, ?)",
        invite_code,
        req.limit_times,
        req.description
    )
    .execute(&pool)
    .await?;

    log_audit(
        &pool,
        &admin,
        "create_invite_code",
        None,
        None,
        Some(&invite_code),
    )
    .await?;

    Ok(Json(ApiResponse::success(CreateInviteCodeResponse {
        invite_code,
        limit_times: req.limit_times,
        description: req.description,
    })))
}

// 查看所有邀请码
async fn list_invite_codes(
    _: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
) -> Result<Json<ApiResponse<ListInviteCodesResponse>>, AppError> {
    let invite_codes =
        sqlx::query_as::<_, InviteCode>("SELECT * FROM invite_codes ORDER BY created_at DESC")
            .fetch_all(&pool)
            .await?;

    Ok(Json(ApiResponse::success(ListInviteCodesResponse {
        invite_codes,
    })))
}

// 获取系统设置
async fn get_settings(
    _: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
) -> Result<Json<ApiResponse<Settings>>, AppError> {
    let settings = sqlx::query_as::<_, Settings>("SELECT * FROM settings WHERE id = 1")
        .fetch_optional(&pool)
        .await?
        .unwrap_or(Settings {
            id: 1,
            invite_code_required: true,
        });

    Ok(Json(ApiResponse::success(settings)))
}

// 更新系统设置
async fn update_settings(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let settings_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM settings WHERE id = 1)")
            .fetch_one(&pool)
            .await?;

    if settings_exists {
        sqlx::query!(
            "UPDATE settings SET invite_code_required = ? WHERE id = 1",
            req.invite_code_required
        )
        .execute(&pool)
        .await?;
    } else {
        sqlx::query!(
            "INSERT INTO settings (id, invite_code_required) VALUES (1, ?)",
            req.invite_code_required
        )
        .execute(&pool)
        .await?;
    }

    log_audit(
        &pool,
        &admin,
        "update_settings",
        Some("settings"),
        Some(1),
        Some(if req.invite_code_required {
            "invite_required=true"
        } else {
            "invite_required=false"
        }),
    )
    .await?;

    Ok(Json(ApiResponse::<()>::message("Settings updated")))
}

// 查看所有用户
async fn list_users(
    _: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
) -> Result<Json<ApiResponse<ListUsersResponse>>, AppError> {
    // 使用 JOIN + GROUP BY 一次性获取用户及其书籍数量，避免 N+1 查询
    let users = sqlx::query_as::<_, AdminUserListItem>(
        r"SELECT u.id AS user_id, u.username, u.created_at,
                  u.total_reading_time, u.status, u.banned_at,
                  COUNT(b.id) AS book_count
           FROM users u
           LEFT JOIN books b ON b.user_id = u.id
           GROUP BY u.id
           ORDER BY u.created_at DESC",
    )
    .fetch_all(&pool)
    .await?;

    Ok(Json(ApiResponse::success(ListUsersResponse { users })))
}

// 重置用户密码
async fn reset_password(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Path(user_id): Path<i64>,
    Json(req): Json<ResetUserPasswordRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let user_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?)")
            .bind(user_id)
            .fetch_one(&pool)
            .await?;

    if !user_exists {
        return Err(AppError::UserNotFound);
    }

    if req.new_password.chars().count() < 6 {
        return Err(AppError::PasswordTooShort);
    }

    let new_password_hash = hash_password(&req.new_password)?;

    sqlx::query!(
        "UPDATE users SET password_hash = ?, failed_login_attempts = 0, locked_until = NULL WHERE \
         id = ?",
        new_password_hash,
        user_id
    )
    .execute(&pool)
    .await?;

    log_audit(
        &pool,
        &admin,
        "reset_password",
        Some("user"),
        Some(user_id),
        None,
    )
    .await?;

    Ok(Json(ApiResponse::<()>::message(
        "User password reset successfully",
    )))
}

// 吊销用户的所有 token
async fn revoke_user_tokens(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Path(user_id): Path<i64>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let user_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?)")
            .bind(user_id)
            .fetch_one(&pool)
            .await?;

    if !user_exists {
        return Err(AppError::UserNotFound);
    }

    // 递增 token_version，使所有已签发的 JWT 失效
    sqlx::query!(
        "UPDATE users SET token_version = token_version + 1 WHERE id = ?",
        user_id
    )
    .execute(&pool)
    .await?;

    log_audit(
        &pool,
        &admin,
        "revoke_tokens",
        Some("user"),
        Some(user_id),
        None,
    )
    .await?;

    Ok(Json(ApiResponse::<()>::message("User tokens revoked")))
}

// 封禁用户：设 status=banned 并递增 token_version 立即踢下线
async fn ban_user(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Path(user_id): Path<i64>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let result = sqlx::query!(
        "UPDATE users SET status = 'banned', banned_at = ?, token_version = token_version + 1 \
         WHERE id = ? AND status != 'banned'",
        now,
        user_id,
    )
    .execute(&pool)
    .await?;

    if result.rows_affected() == 0 {
        // 可能不存在或已经被封禁
        let exists =
            sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?)")
                .bind(user_id)
                .fetch_one(&pool)
                .await?;
        if !exists {
            return Err(AppError::UserNotFound);
        }
        // 已被封禁，幂等返回成功
    }

    log_audit(&pool, &admin, "ban_user", Some("user"), Some(user_id), None).await?;

    Ok(Json(ApiResponse::<()>::message("User banned")))
}

// 解封用户
async fn unban_user(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Path(user_id): Path<i64>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let result = sqlx::query!(
        "UPDATE users SET status = 'active', banned_at = NULL, failed_login_attempts = 0, \
         locked_until = NULL WHERE id = ? AND status = 'banned'",
        user_id,
    )
    .execute(&pool)
    .await?;

    if result.rows_affected() == 0 {
        let exists =
            sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?)")
                .bind(user_id)
                .fetch_one(&pool)
                .await?;
        if !exists {
            return Err(AppError::UserNotFound);
        }
        // 非封禁状态，幂等返回成功
    }

    log_audit(
        &pool,
        &admin,
        "unban_user",
        Some("user"),
        Some(user_id),
        None,
    )
    .await?;

    Ok(Json(ApiResponse::<()>::message("User unbanned")))
}

// 列出所有管理员
async fn list_admins(
    _: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
) -> Result<Json<ApiResponse<ListAdminsResponse>>, AppError> {
    let admins =
        sqlx::query!("SELECT id, username, created_at FROM admin ORDER BY created_at DESC")
            .fetch_all(&pool)
            .await?;

    let list: Vec<AdminListItem> = admins
        .into_iter()
        .map(|a| AdminListItem {
            admin_id: a.id,
            username: a.username,
            created_at: a.created_at,
        })
        .collect();

    Ok(Json(ApiResponse::success(ListAdminsResponse {
        admins: list,
    })))
}

// 创建新管理员
async fn create_admin(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Json(req): Json<CreateAdminRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // 验证用户名格式
    let username_len = req.username.chars().count();
    if !(2..=20).contains(&username_len) {
        return Err(AppError::UsernameLengthInvalid);
    }

    // 验证密码格式
    if req.password.chars().count() < 6 {
        return Err(AppError::PasswordTooShort);
    }

    // 检查用户名唯一
    let exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM admin WHERE username = ?)")
            .bind(&req.username)
            .fetch_one(&pool)
            .await?;
    if exists {
        return Err(AppError::AdminUsernameAlreadyExists);
    }

    let password_hash = hash_password(&req.password)?;

    let new_admin_id = sqlx::query!(
        "INSERT INTO admin (username, password_hash) VALUES (?, ?)",
        req.username,
        password_hash
    )
    .execute(&pool)
    .await?
    .last_insert_rowid();

    log_audit(
        &pool,
        &admin,
        "create_admin",
        Some("admin"),
        Some(new_admin_id),
        Some(&req.username),
    )
    .await?;

    Ok(Json(ApiResponse::<()>::message("Admin created")))
}

// 查看审计日志
async fn list_audit_logs(
    _: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ApiResponse<ListAuditLogsResponse>>, AppError> {
    let page = params.page.unwrap_or(1);
    #[allow(clippy::cast_sign_loss)]
    let limit = params.limit.unwrap_or(20).min(100);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let offset = i64::from((page - 1) * limit);

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_logs")
        .fetch_one(&pool)
        .await?;

    let logs = sqlx::query_as::<_, AuditLog>(
        "SELECT * FROM audit_logs ORDER BY created_at DESC LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&pool)
    .await?;

    Ok(Json(ApiResponse::success(ListAuditLogsResponse {
        total,
        logs,
    })))
}

// 全局备份：把 db 文件 + 全部书籍文件打包成 zip 供下载
async fn backup_global(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
) -> Result<Response, AppError> {
    use std::io::Write;

    // 强制 WAL 落盘，保证备份一致性
    let _ = sqlx::query("PRAGMA wal_checkpoint(FULL)")
        .execute(&pool)
        .await;

    log_audit(&pool, &admin, "backup_global", None, None, None).await?;

    let mut zip_buf: Vec<u8> = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        // 写入数据库文件（从配置的 url 解析路径；支持 sqlite: 前缀）
        let db_url = &config.db.url;
        let db_path = db_url
            .trim_start_matches("sqlite:")
            .trim_start_matches("sqlite://");
        match tokio::fs::read(db_path).await {
            Ok(bytes) => {
                zip.start_file("reader.db", opts)
                    .map_err(|e| AppError::Internal(format!("zip error: {e}")))?;
                zip.write_all(&bytes)
                    .map_err(|e| AppError::Internal(format!("zip error: {e}")))?;
            },
            Err(e) => {
                tracing::warn!(error = %e, db = %db_path, "skip db file in global backup");
            },
        }

        // 写入 books 目录所有文件
        let book_dir = &config.storage.book_dir;
        let mut entries = match tokio::fs::read_dir(book_dir).await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = %e, dir = %book_dir.display(), "skip book_dir in backup");
                zip.finish()
                    .map_err(|e| AppError::Internal(format!("zip finalize: {e}")))?;
                return Ok(finalize_backup(zip_buf));
            },
        };

        while let Some(entry) = entries.next_entry().await.map_err(AppError::Io)? {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let entry_name = format!("books/{name}");
            if zip.start_file(&entry_name, opts).is_err() {
                continue;
            }
            if let Ok(bytes) = tokio::fs::read(&path).await {
                let _ = zip.write_all(&bytes);
            }
        }

        zip.finish()
            .map_err(|e| AppError::Internal(format!("zip finalize: {e}")))?;
    }

    Ok(finalize_backup(zip_buf))
}

fn finalize_backup(zip_buf: Vec<u8>) -> Response {
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let filename = format!("reader-backup-{timestamp}.zip");
    Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from(zip_buf))
        .unwrap()
}
