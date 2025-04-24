use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use sqlx::{Pool, Sqlite};

use crate::{
    auth::AuthAdmin,
    error::{ApiResponse, AppError},
    models::{
        AdminUserListItem, CreateInviteCodeRequest, InviteCode, ResetUserPasswordRequest, Settings,
        UpdateSettingsRequest, User,
    },
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
}

// 检查是否已设置管理员
async fn check_setup(
    State(pool): State<Pool<Sqlite>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    // 检查是否已有管理员
    let admin_exists = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM admin)")
        .fetch_one(&pool)
        .await?;

    Ok(Json(ApiResponse::success(serde_json::json!({
        "is_setup": admin_exists
    }))))
}

// 创建邀请码
async fn create_invite_code(
    _: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Json(req): Json<CreateInviteCodeRequest>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    // 生成邀请码
    let invite_code = generate_invite_code();

    // 插入数据库
    sqlx::query!(
        "INSERT INTO invite_codes (code, limit_times, description) VALUES (?, ?, ?)",
        invite_code,
        req.limit_times,
        req.description
    )
    .execute(&pool)
    .await?;

    // 返回创建的邀请码
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
    // 查询所有邀请码
    let invite_codes =
        sqlx::query_as::<_, InviteCode>("SELECT * FROM invite_codes ORDER BY created_at DESC")
            .fetch_all(&pool)
            .await?;

    // 返回邀请码列表
    Ok(Json(ApiResponse::success(serde_json::json!({
        "invite_codes": invite_codes
    }))))
}

// 获取系统设置
async fn get_settings(
    _: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
) -> Result<Json<ApiResponse<Settings>>, AppError> {
    // 查询系统设置
    let settings = sqlx::query_as::<_, Settings>("SELECT * FROM settings WHERE id = 1")
        .fetch_optional(&pool)
        .await?
        .unwrap_or(Settings {
            id: 1,
            invite_code_required: true,
        });

    // 返回系统设置
    Ok(Json(ApiResponse::success(settings)))
}

// 更新系统设置
async fn update_settings(
    _: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // 检查是否已有设置
    let settings_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM settings WHERE id = 1)")
            .fetch_one(&pool)
            .await?;

    if settings_exists {
        // 更新设置
        sqlx::query!(
            "UPDATE settings SET invite_code_required = ? WHERE id = 1",
            req.invite_code_required
        )
        .execute(&pool)
        .await?;
    } else {
        // 创建设置
        sqlx::query!(
            "INSERT INTO settings (id, invite_code_required) VALUES (1, ?)",
            req.invite_code_required
        )
        .execute(&pool)
        .await?;
    }

    // 返回成功信息
    Ok(Json(ApiResponse::<()>::message("设置已更新")))
}

// 查看所有用户
async fn list_users(
    _: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    // 查询所有用户
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

    // 返回用户列表
    Ok(Json(ApiResponse::success(serde_json::json!({
        "users": user_list
    }))))
}

// 重置用户密码
async fn reset_password(
    _: AuthAdmin,
    State(pool): State<Pool<Sqlite>>,
    Path(user_id): Path<i64>,
    Json(req): Json<ResetUserPasswordRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // 检查用户是否存在
    let user_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE id = ?)")
            .bind(user_id)
            .fetch_one(&pool)
            .await?;

    if !user_exists {
        return Err(AppError::NotFound("用户不存在".to_string()));
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
        user_id
    )
    .execute(&pool)
    .await?;

    // 返回成功信息
    Ok(Json(ApiResponse::<()>::message("用户密码重置成功")))
}
