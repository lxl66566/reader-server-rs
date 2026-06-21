use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{Pool, Sqlite};

use crate::{
    auth::AuthUser,
    error::{ApiResponse, AppError},
    models::{HeartbeatRequest, HeartbeatResponse, ReadingSettings, UpdateReadingSettingsRequest},
};

// 阅读路由
pub fn routes() -> Router<crate::app::AppState> {
    Router::new()
        .route(
            "/settings",
            get(get_reading_settings).put(update_reading_settings),
        )
        .route("/heartbeat", post(process_heartbeat))
}

// 获取阅读设置
async fn get_reading_settings(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
) -> Result<Json<ApiResponse<ReadingSettings>>, AppError> {
    // 查询用户阅读设置
    let settings =
        sqlx::query_as::<_, ReadingSettings>("SELECT * FROM reading_settings WHERE user_id = ?")
            .bind(auth.user_id)
            .fetch_optional(&pool)
            .await?;

    // 如果设置不存在，创建默认设置
    let settings = if let Some(settings) = settings {
        settings
    } else {
        // 创建默认设置
        let id = sqlx::query!(
            "INSERT OR IGNORE INTO reading_settings (user_id) VALUES (?)",
            auth.user_id
        )
        .execute(&pool)
        .await?
        .last_insert_rowid();

        ReadingSettings {
            id,
            user_id: auth.user_id,
            font_size: 18,
            background_color: "#F5F5DC".to_string(),
            text_color: "#000000".to_string(),
            line_height: 1.5,
            letter_spacing: 0.05,
            paragraph_spacing: 1.2,
            reading_width: 800,
            text_indent: 2.0,
            simplified_chinese: true,
        }
    };

    Ok(Json(ApiResponse::success(settings)))
}

// 更新阅读设置
async fn update_reading_settings(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Json(req): Json<UpdateReadingSettingsRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // 检查设置是否存在
    let settings_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM reading_settings WHERE user_id = ?)",
    )
    .bind(auth.user_id)
    .fetch_one(&pool)
    .await?;

    if !settings_exists {
        // 如果设置不存在，创建默认设置
        sqlx::query!(
            "INSERT INTO reading_settings (user_id) VALUES (?)",
            auth.user_id
        )
        .execute(&pool)
        .await?;
    }

    // 构建动态更新 SQL（使用 separated 自动处理逗号分隔）
    let mut query_builder: sqlx::QueryBuilder<sqlx::Sqlite> =
        sqlx::QueryBuilder::new("UPDATE reading_settings SET ");
    let mut sep = query_builder.separated(", ");

    let mut has_update = false;
    if let Some(v) = req.font_size {
        sep.push("font_size = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.background_color {
        sep.push("background_color = ")
            .push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.text_color {
        sep.push("text_color = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.line_height {
        sep.push("line_height = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.letter_spacing {
        sep.push("letter_spacing = ")
            .push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.paragraph_spacing {
        sep.push("paragraph_spacing = ")
            .push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.reading_width {
        sep.push("reading_width = ")
            .push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.text_indent {
        sep.push("text_indent = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.simplified_chinese {
        sep.push("simplified_chinese = ")
            .push_bind_unseparated(v);
        has_update = true;
    }

    if !has_update {
        return Ok(Json(ApiResponse::<()>::message("No updates")));
    }

    query_builder
        .push(" WHERE user_id = ")
        .push_bind(auth.user_id);
    query_builder.build().execute(&pool).await?;

    Ok(Json(ApiResponse::<()>::message("Settings updated")))
}

// 处理心跳包
async fn process_heartbeat(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Json(req): Json<HeartbeatRequest>,
) -> Result<Json<ApiResponse<HeartbeatResponse>>, AppError> {
    // 校验 position 非负
    if req.position < 0 {
        return Err(AppError::InvalidPosition);
    }

    // 检查书籍是否存在
    let book = sqlx::query!(
        "SELECT user_id, is_public FROM books WHERE id = ?",
        req.book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::BookNotFound)?;

    // 检查权限
    if book.user_id != auth.user_id && !book.is_public {
        return Err(AppError::BookAccessDenied);
    }

    // 获取当前进度
    let progress = sqlx::query!(
        "SELECT position, reading_time, last_read_at, last_device_id 
         FROM reading_progress 
         WHERE user_id = ? AND book_id = ?",
        auth.user_id,
        req.book_id
    )
    .fetch_optional(&pool)
    .await?;

    let now = Utc::now();
    let current_device_id = req.device_id.clone();
    let now_str = now.to_rfc3339_opts(SecondsFormat::Millis, true);

    // 如果没有阅读进度记录，创建一个
    if progress.is_none() {
        sqlx::query!(
            "INSERT INTO reading_progress 
             (user_id, book_id, position, last_read_at, last_device_id) 
             VALUES (?, ?, ?, ?, ?)",
            auth.user_id,
            req.book_id,
            req.position,
            now_str,
            current_device_id
        )
        .execute(&pool)
        .await?;

        return Ok(Json(ApiResponse::success(HeartbeatResponse {
            synced: true,
            position: req.position,
            reading_time: 0,
        })));
    }

    let progress = progress.unwrap();
    let last_device_id = progress.last_device_id;
    let last_read_at = progress.last_read_at;

    // 检查设备ID是否相同
    let is_same_device = last_device_id
        .as_deref()
        .is_none_or(|id| id == current_device_id);

    // 如果设备不同，返回服务器保存的进度
    if !is_same_device {
        return Ok(Json(ApiResponse::success(HeartbeatResponse {
            synced: false,
            position: progress.position,
            reading_time: progress.reading_time,
        })));
    }

    // 计算阅读时间增量；解析失败时跳过（不累加），避免 panic
    let mut reading_time_increment = 0;
    if let Some(last_time) = last_read_at.as_deref() {
        if let Ok(last_dt) = last_time.parse::<DateTime<Utc>>() {
            let seconds = now.signed_duration_since(last_dt).num_seconds();
            // 时间间隔在合理范围内（少于 30 秒），计为阅读时间
            if seconds > 0 && seconds < 30 {
                reading_time_increment = seconds;
            }
        }
    }

    // 使用乐观锁更新阅读进度：WHERE 条件包含 last_read_at 原始值，
    // 若在此期间被其他并发心跳修改，rows_affected == 0。
    // 使用 reading_time = reading_time + ? 确保原子累加。
    let mut qb = sqlx::QueryBuilder::new("UPDATE reading_progress SET position = ");
    qb.push_bind(req.position);
    qb.push(", reading_time = reading_time + ");
    qb.push_bind(reading_time_increment);
    qb.push(", last_read_at = ");
    qb.push_bind(&now_str);
    qb.push(", last_device_id = ");
    qb.push_bind(&current_device_id);
    qb.push(" WHERE user_id = ");
    qb.push_bind(auth.user_id);
    qb.push(" AND book_id = ");
    qb.push_bind(req.book_id);
    qb.push(" AND ");
    match &last_read_at {
        Some(t) => {
            qb.push("last_read_at = ");
            qb.push_bind(t);
        }
        None => {
            qb.push("last_read_at IS NULL");
        }
    }

    let result = qb.build().execute(&pool).await?;

    // 乐观锁竞争失败：并发请求已抢先更新，返回当前最新进度（设备相同 → synced=true）
    if result.rows_affected() == 0 {
        let current = sqlx::query!(
            "SELECT position, reading_time FROM reading_progress WHERE user_id = ? AND book_id = ?",
            auth.user_id,
            req.book_id
        )
        .fetch_one(&pool)
        .await?;

        return Ok(Json(ApiResponse::success(HeartbeatResponse {
            synced: true,
            position: current.position,
            reading_time: current.reading_time,
        })));
    }

    // 更新用户总阅读时间
    if reading_time_increment > 0 {
        sqlx::query!(
            "UPDATE users 
             SET total_reading_time = total_reading_time + ? 
             WHERE id = ?",
            reading_time_increment,
            auth.user_id
        )
        .execute(&pool)
        .await?;
    }

    let new_reading_time = progress.reading_time + reading_time_increment;

    // 返回同步状态和最新阅读时间
    Ok(Json(ApiResponse::success(HeartbeatResponse {
        synced: true,
        position: req.position,
        reading_time: new_reading_time,
    })))
}
