use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use sqlx::{Pool, Sqlite};

use crate::{
    auth::AuthAdmin,
    error::{ApiResponse, AppError},
    models::{
        AdminListItem, AdminUserListItem, AuditLog, CreateAdminRequest, CreateInviteCodeRequest,
        InviteCode, ResetUserPasswordRequest, Settings, UpdateSettingsRequest, User,
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
        .route("/admins", get(list_admins).post(create_admin))
        .route("/audit_logs", get(list_audit_logs))
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
        "INSERT INTO audit_logs (admin_id, admin_username, action, target_type, target_id, detail) \
         VALUES (?, ?, ?, ?, ?, ?)",
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
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let admin_exists = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM admin)")
        .fetch_one(&pool)
        .await?;

    Ok(Json(ApiResponse::success(serde_json::json!({
        "is_setup": admin_exists
    }))))
}

// 创建邀请码
async fn create_invite_code(
    admin: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Json(req): Json<CreateInviteCodeRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
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

    Ok(Json(ApiResponse::success(serde_json::json!({
        "invite_code": invite_code,
        "limit_times": req.limit_times,
        "description": req.description
    }))))
}

// 查看所有邀请码
async fn list_invite_codes(
    _: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let invite_codes =
        sqlx::query_as::<_, InviteCode>("SELECT * FROM invite_codes ORDER BY created_at DESC")
            .fetch_all(&pool)
            .await?;

    Ok(Json(ApiResponse::success(serde_json::json!({
        "invite_codes": invite_codes
    }))))
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
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let users = sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY created_at DESC")
        .fetch_all(&pool)
        .await?;

    // 查询每个用户的书籍数量
    let mut user_list = Vec::new();
    for user in users {
        let book_count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM books WHERE user_id = ?")
                .bind(user.id)
                .fetch_one(&pool)
                .await?;

        user_list.push(AdminUserListItem {
            user_id: user.id,
            username: user.username,
            created_at: user.created_at,
            book_count,
            total_reading_time: user.total_reading_time,
        });
    }

    Ok(Json(ApiResponse::success(serde_json::json!({
        "users": user_list
    }))))
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
        "UPDATE users SET password_hash = ?, failed_login_attempts = 0, locked_until = NULL WHERE id = ?",
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

// 列出所有管理员
async fn list_admins(
    _: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let admins = sqlx::query!("SELECT id, username, created_at FROM admin ORDER BY created_at DESC")
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

    Ok(Json(ApiResponse::success(serde_json::json!({
        "admins": list
    }))))
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
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    let page = params.page.unwrap_or(1);
    #[allow(clippy::cast_sign_loss)]
    let limit = params.limit.unwrap_or(20).min(100);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let offset = ((page - 1) * limit) as i64;

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

    Ok(Json(ApiResponse::success(serde_json::json!({
        "total": total,
        "logs": logs
    }))))
}
