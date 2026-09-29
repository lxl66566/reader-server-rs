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
        ResetUserPasswordRequest, Settings, UpdateSettingsRequest, UserStatus,
    },
    routes::{auth::validate_username, books::PaginationParams},
    utils::{generate_invite_code, generate_uuid, hash_password},
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

    let new_password_hash = hash_password(&req.new_password)?;

    // 重置密码同时解除锁定并递增 token_version，使旧 JWT 立即失效
    sqlx::query!(
        "UPDATE users SET password_hash = ?, failed_login_attempts = 0, locked_until = NULL, \
         token_version = token_version + 1 WHERE id = ?",
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
        "UPDATE users SET status = ?, banned_at = ?, token_version = token_version + 1 WHERE id = \
         ? AND status != ?",
        UserStatus::Banned,
        now,
        user_id,
        UserStatus::Banned,
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
        "UPDATE users SET status = ?, banned_at = NULL, failed_login_attempts = 0, locked_until = \
         NULL WHERE id = ? AND status = ?",
        UserStatus::Active,
        user_id,
        UserStatus::Banned,
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
    // 验证用户名格式（长度 + 字符白名单）
    validate_username(&req.username)?;

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
    // 归一化分页参数：page 最小为 1，limit 上限 100
    let (limit, offset) = params.normalize(20, 100);

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

// 全局备份：数据库一致性快照 + 全部书籍文件打包成 zip 供下载。
// 任何组成部分缺失/失败都显式报错，绝不产出“看似成功实则缺文件”的备份。
async fn backup_global(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
) -> Result<Response, AppError> {
    use std::io::Write;

    log_audit(&pool, &admin, "backup_global", None, None, None).await?;

    // 用 VACUUM INTO 生成数据库一致性快照（SQLite 在线备份）：
    // 直接读 db 文件在 WAL 模式下会漏掉未 checkpoint 的数据，且与并发写入竞争；
    // 此前从 db url 剥离前缀的顺序也是错的（先剥 "sqlite:" 后 "sqlite://" 永不命中），
    // 快照方式不再依赖路径解析
    let snapshot_path = std::env::temp_dir().join(format!("reader-backup-{}.db", generate_uuid()));
    let snapshot_result: Result<Vec<u8>, AppError> = async {
        sqlx::query("VACUUM INTO ?")
            .bind(snapshot_path.to_string_lossy().as_ref())
            .execute(&pool)
            .await?;
        tokio::fs::read(&snapshot_path).await.map_err(AppError::Io)
    }
    .await;
    // 无论成败都清理临时快照文件
    let _ = tokio::fs::remove_file(&snapshot_path).await;
    // 数据库是备份的核心，快照失败必须显式拒绝
    let db_bytes = snapshot_result?;

    let mut zip_buf: Vec<u8> = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut zip_buf));
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        zip.start_file("reader.db", opts)
            .map_err(|e| AppError::Internal(format!("zip error: {e}")))?;
        zip.write_all(&db_bytes)
            .map_err(|e| AppError::Internal(format!("zip error: {e}")))?;

        // 写入 books 目录所有文件；任何文件读取/写入失败都记录下来，
        // 最后整体报错拒绝，不让调用方在不知情的情况下拿到残缺备份
        let book_dir = &config.storage.book_dir;
        let mut entries = tokio::fs::read_dir(book_dir).await.map_err(AppError::Io)?;
        let mut failed: Vec<String> = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(AppError::Io)? {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                failed.push(path.display().to_string());
                continue;
            };
            let entry_name = format!("books/{name}");
            let archive_result: Result<(), AppError> = async {
                let bytes = tokio::fs::read(&path).await.map_err(AppError::Io)?;
                zip.start_file(&entry_name, opts)
                    .map_err(|e| AppError::Internal(format!("zip error: {e}")))?;
                zip.write_all(&bytes)
                    .map_err(|e| AppError::Internal(format!("zip error: {e}")))?;
                Ok(())
            }
            .await;
            if let Err(e) = archive_result {
                tracing::error!(error = %e, file = %path.display(), "backup: failed to archive book file");
                failed.push(path.display().to_string());
            }
        }

        zip.finish()
            .map_err(|e| AppError::Internal(format!("zip finalize: {e}")))?;

        if !failed.is_empty() {
            // 失败详情已记入日志；向客户端只返回固定文案（Internal 的统一行为）
            return Err(AppError::Internal(format!(
                "backup incomplete: {} file(s) failed to archive",
                failed.len()
            )));
        }
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
