use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State, multipart::Multipart},
    response::Response,
    routing::{get, post, put},
};
use chrono::{SecondsFormat, Utc};
use sqlx::{Pool, Sqlite};
use tokio::io::AsyncWriteExt as _;

use crate::{
    auth::AuthAdmin,
    config::Config,
    error::{ApiResponse, AppError},
    models::{
        AdminListItem, AdminSetReadingTimeRequest, AdminUserListItem, AuditLog,
        ChangePasswordRequest, CheckSetupResponse, CreateAdminRequest, CreateInviteCodeRequest,
        CreateInviteCodeResponse, DEFAULT_INVITE_CODE_REQUIRED, ImportUserResponse, InviteCode,
        ListAdminsResponse, ListAuditLogsResponse, ListInviteCodesResponse, ListUsersResponse,
        ResetUserPasswordRequest, Settings, UpdateSettingsRequest, UserStatus,
    },
    routes::{
        auth::{validate_password, validate_username},
        books::{PaginationParams, map_multipart_err},
        reading::set_reading_time,
    },
    user_data::{self, ExportScope},
    utils::{generate_invite_code, generate_uuid, hash_password, verify_password},
};

// 管理员路由
pub fn routes() -> Router<crate::app::AppState> {
    Router::new()
        .route("/check_setup", get(check_setup))
        .route("/password", post(change_password))
        .route("/invite_code", post(create_invite_code))
        .route("/invite_codes", get(list_invite_codes))
        .route("/settings", get(get_settings).put(update_settings))
        .route("/users", get(list_users))
        .route(
            "/users/import",
            post(import_user).layer(DefaultBodyLimit::max(IMPORT_BODY_LIMIT)),
        )
        .route("/users/{user_id}/reset_password", post(reset_password))
        .route("/users/{user_id}/reading_time", put(admin_set_reading_time))
        .route("/users/{user_id}/revoke_tokens", post(revoke_user_tokens))
        .route("/users/{user_id}/ban", post(ban_user))
        .route("/users/{user_id}/unban", post(unban_user))
        .route("/users/{user_id}/export", get(export_user))
        .route("/admins", get(list_admins).post(create_admin))
        .route("/audit_logs", get(list_audit_logs))
        .route("/backup", get(backup_global))
}

/// 导入请求体上限：导出 zip 打包用户全部书籍，远超单书上传上限；
/// handler 内流式落盘，内存占用与包大小无关
const IMPORT_BODY_LIMIT: usize = 4 * 1024 * 1024 * 1024;

/// 审计文本字段入库前的截断上限（按字符计）
const AUDIT_TEXT_MAX_CHARS: usize = 256;

/// 按字符边界截断，避免切在 UTF-8 多字节序列中间
fn truncate_chars(input: Option<&str>, max_chars: usize) -> Option<String> {
    input.map(|s| s.chars().take(max_chars).collect())
}

/// 记录审计日志
async fn log_audit(
    pool: &Pool<Sqlite>,
    admin: &AuthAdmin,
    action: &str,
    target_type: Option<&str>,
    target_id: Option<i64>,
    detail: Option<&str>,
) {
    log_audit_event(
        pool,
        Some(admin.admin_id),
        Some(&admin.username),
        action,
        target_type,
        target_id,
        detail,
    )
    .await;
}

/// 记录审计日志（泛化版，best-effort）：登录类事件发生在认证之前/之外，
/// 可能没有操作管理员，此时 admin_id / admin_username 传 None。
/// 写入失败仅告警、不影响业务响应；文本字段按字符截断到 256
pub(crate) async fn log_audit_event(
    pool: &Pool<Sqlite>,
    admin_id: Option<i64>,
    admin_username: Option<&str>,
    action: &str,
    target_type: Option<&str>,
    target_id: Option<i64>,
    detail: Option<&str>,
) {
    let result = sqlx::query!(
        "INSERT INTO audit_logs (admin_id, admin_username, action, target_type, target_id, \
         detail) VALUES (?, ?, ?, ?, ?, ?)",
        admin_id,
        truncate_chars(admin_username, AUDIT_TEXT_MAX_CHARS),
        action,
        target_type,
        target_id,
        truncate_chars(detail, AUDIT_TEXT_MAX_CHARS),
    )
    .execute(pool)
    .await;
    if let Err(e) = result {
        tracing::warn!(error = %e, action, "failed to write audit log");
    }
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

// 管理员修改密码：验证旧密码，写入新哈希并递增 token_version，
// 使所有已签发的旧 admin JWT 立即失效（含当前请求使用的 token）
async fn change_password(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let row = sqlx::query!(
        "SELECT password_hash FROM admin WHERE id = ?",
        admin.admin_id
    )
    .fetch_one(&pool)
    .await?;

    // 验证旧密码
    if !verify_password(&req.old_password, &row.password_hash).await? {
        return Err(AppError::IncorrectOldPassword);
    }

    // 校验新密码非空
    validate_password(&req.new_password)?;

    let new_password_hash = hash_password(&req.new_password).await?;

    sqlx::query!(
        "UPDATE admin SET password_hash = ?, token_version = token_version + 1 WHERE id = ?",
        new_password_hash,
        admin.admin_id
    )
    .execute(&pool)
    .await?;

    log_audit(
        &pool,
        &admin,
        "change_password",
        Some("admin"),
        Some(admin.admin_id),
        None,
    )
    .await;

    Ok(Json(ApiResponse::<()>::message(
        "Password changed successfully",
    )))
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
    .await;

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
            // 与注册侧共用同一缺省，避免“注册不需邀请码 / 管理端显示需要”的矛盾
            invite_code_required: DEFAULT_INVITE_CODE_REQUIRED,
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
    .await;

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

// 导出指定用户的全部数据（迁移范围：含密码哈希与书籍文件），用于跨实例迁移。
// 任何书籍文件缺失/读取失败都整体报错，绝不产出残缺包
async fn export_user(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Path(user_id): Path<i64>,
) -> Result<Response, AppError> {
    log_audit(
        &pool,
        &admin,
        "export_user",
        Some("user"),
        Some(user_id),
        None,
    )
    .await;

    let (zip_path, filename) =
        user_data::build_user_export_zip(&pool, user_id, ExportScope::Migration).await?;

    let content_disposition = format!(
        "attachment; filename*=UTF-8''{}",
        user_data::percent_encode_filename(&filename)
    );
    crate::utils::zip::stream_temp_zip(zip_path, content_disposition).await
}

// 导入用户数据包：恢复账号 + 书籍 + 全部阅读数据为新用户。
// username / password 缺省（或留空）时使用导出包内的原值
async fn import_user(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    mut multipart: Multipart,
) -> Result<Json<ApiResponse<ImportUserResponse>>, AppError> {
    let zip_path = std::env::temp_dir().join(format!("reader-import-{}.zip", generate_uuid()));

    let result = async {
        // zip 流式落盘（可能打包用户全部书籍），内存占用与包大小无关
        let mut have_file = false;
        let mut username = None;
        let mut password = None;
        while let Some(mut field) = multipart
            .next_field()
            .await
            .map_err(|e| map_multipart_err(&e, AppError::FormParseError))?
        {
            let name = field.name().unwrap_or("").to_string();
            match name.as_str() {
                "file" => {
                    let mut file = tokio::fs::File::create(&zip_path)
                        .await
                        .map_err(AppError::Io)?;
                    let mut written: usize = 0;
                    while let Some(chunk) = field.chunk().await.map_err(|e| {
                        map_multipart_err(&e, |s| AppError::FieldReadError("file".to_string(), s))
                    })? {
                        written += chunk.len();
                        // body limit 已限制总量，此处防御性双保险
                        if written > IMPORT_BODY_LIMIT {
                            return Err(AppError::FileTooLarge);
                        }
                        file.write_all(&chunk).await.map_err(AppError::Io)?;
                    }
                    // 落盘后再交给 spawn_blocking；省略 flush 有读到截断文件的风险
                    file.flush().await.map_err(AppError::Io)?;
                    drop(file);
                    have_file = written > 0;
                },
                "username" => {
                    username = Some(field.text().await.map_err(|e| {
                        AppError::FieldReadError("username".to_string(), e.to_string())
                    })?);
                },
                "password" => {
                    password = Some(field.text().await.map_err(|e| {
                        AppError::FieldReadError("password".to_string(), e.to_string())
                    })?);
                },
                _ => {},
            }
        }
        if !have_file {
            return Err(AppError::FileEmpty);
        }

        user_data::restore_user_export(&pool, &config, &zip_path, username, password).await
    }
    .await;
    // 无论成败都清理临时 zip
    let _ = tokio::fs::remove_file(&zip_path).await;
    let outcome = result?;

    log_audit(
        &pool,
        &admin,
        "import_user",
        Some("user"),
        Some(outcome.user_id),
        Some(&outcome.username),
    )
    .await;

    Ok(Json(ApiResponse::success(outcome)))
}

// 管理员修正任意用户在某本书上的累计阅读时长
async fn admin_set_reading_time(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Path(user_id): Path<i64>,
    Json(req): Json<AdminSetReadingTimeRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let user_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?)")
            .bind(user_id)
            .fetch_one(&pool)
            .await?;
    if !user_exists {
        return Err(AppError::UserNotFound);
    }

    let book_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM books WHERE id = ?)")
            .bind(req.book_id)
            .fetch_one(&pool)
            .await?;
    if !book_exists {
        return Err(AppError::BookNotFound);
    }

    set_reading_time(&pool, user_id, req.book_id, req.reading_time).await?;

    log_audit(
        &pool,
        &admin,
        "set_reading_time",
        Some("user"),
        Some(user_id),
        Some(&format!(
            "book_id={} reading_time={}",
            req.book_id, req.reading_time
        )),
    )
    .await;

    Ok(Json(ApiResponse::<()>::message("Reading time updated")))
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

    // 校验新密码非空
    validate_password(&req.new_password)?;

    let new_password_hash = hash_password(&req.new_password).await?;

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
    .await;

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
    .await;

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

    log_audit(&pool, &admin, "ban_user", Some("user"), Some(user_id), None).await;

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
    .await;

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

    // 验证密码非空
    validate_password(&req.password)?;

    let password_hash = hash_password(&req.password).await?;

    // 直接 INSERT，依赖 UNIQUE 约束兜底同名冲突，消除“先查后插”的 TOCTOU；
    // 并发下败者映射为既有的用户名已存在错误（400）而不是 500
    let new_admin_id = match sqlx::query!(
        "INSERT INTO admin (username, password_hash) VALUES (?, ?)",
        req.username,
        password_hash
    )
    .execute(&pool)
    .await
    {
        Ok(result) => result.last_insert_rowid(),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(AppError::AdminUsernameAlreadyExists);
        },
        Err(e) => return Err(e.into()),
    };

    log_audit(
        &pool,
        &admin,
        "create_admin",
        Some("admin"),
        Some(new_admin_id),
        Some(&req.username),
    )
    .await;

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
// zip 写入临时文件后流式回传（书籍文件 io::copy 搬运），内存占用与书数量无关。
async fn backup_global(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
) -> Result<Response, AppError> {
    log_audit(&pool, &admin, "backup_global", None, None, None).await;

    // 用 VACUUM INTO 生成数据库一致性快照（SQLite 在线备份）：
    // 直接读 db 文件在 WAL 模式下会漏掉未 checkpoint 的数据，且与并发写入竞争；
    // 此前从 db url 剥离前缀的顺序也是错的（先剥 "sqlite:" 后 "sqlite://" 永不命中），
    // 快照方式不再依赖路径解析
    let snapshot_path = std::env::temp_dir().join(format!("reader-backup-{}.db", generate_uuid()));
    let snapshot_result: Result<(), AppError> = async {
        sqlx::query("VACUUM INTO ?")
            .bind(snapshot_path.to_string_lossy().as_ref())
            .execute(&pool)
            .await?;
        Ok(())
    }
    .await;
    // 数据库是备份的核心，快照失败必须显式拒绝
    snapshot_result?;

    let book_dir = config.storage.book_dir.clone();
    let snapshot_for_zip = snapshot_path.clone();
    let zip_result = crate::utils::zip::write_temp_zip(move |zip| {
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let zip_err = crate::utils::zip::zip_write_err;

        zip.start_file("reader.db", opts).map_err(zip_err)?;
        let mut db_file = std::fs::File::open(&snapshot_for_zip).map_err(AppError::Io)?;
        std::io::copy(&mut db_file, zip).map_err(AppError::Io)?;

        // 写入 books 目录所有文件；任何文件读取/写入失败都记录下来，
        // 最后整体报错拒绝，不让调用方在不知情的情况下拿到残缺备份
        let mut failed: Vec<String> = Vec::new();
        for entry in std::fs::read_dir(&book_dir).map_err(AppError::Io)? {
            let entry = entry.map_err(AppError::Io)?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                failed.push(path.display().to_string());
                continue;
            };
            let entry_name = format!("books/{name}");
            let archive_result: Result<(), AppError> = (|| {
                let mut file = std::fs::File::open(&path).map_err(AppError::Io)?;
                zip.start_file(&entry_name, opts).map_err(zip_err)?;
                std::io::copy(&mut file, zip).map_err(AppError::Io)?;
                Ok(())
            })();
            if let Err(e) = archive_result {
                tracing::error!(error = %e, file = %path.display(), "backup: failed to archive book file");
                failed.push(path.display().to_string());
            }
        }

        if !failed.is_empty() {
            // 失败详情已记入日志；向客户端只返回固定文案（Internal 的统一行为）
            return Err(AppError::Internal(format!(
                "backup incomplete: {} file(s) failed to archive",
                failed.len()
            )));
        }
        Ok(())
    })
    .await;
    // 无论成败都清理临时快照文件
    let _ = tokio::fs::remove_file(&snapshot_path).await;
    let (zip_path, _) = zip_result?;

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let filename = format!("reader-backup-{timestamp}.zip");
    crate::utils::zip::stream_temp_zip(zip_path, format!("attachment; filename=\"{filename}\""))
        .await
}
