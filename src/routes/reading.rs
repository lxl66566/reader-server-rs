use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
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
            "INSERT INTO reading_settings (user_id) VALUES (?)",
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

    // 构建更新SQL
    let mut sql_parts = Vec::new();
    let mut params: Vec<(String, _)> = Vec::new();

    if let Some(font_size) = req.font_size {
        sql_parts.push("font_size = ?".to_string());
        params.push(("font_size".to_string(), font_size.to_string()));
    }

    if let Some(background_color) = req.background_color {
        sql_parts.push("background_color = ?".to_string());
        params.push(("background_color".to_string(), background_color.to_string()));
    }

    if let Some(text_color) = req.text_color {
        sql_parts.push("text_color = ?".to_string());
        params.push(("text_color".to_string(), text_color.to_string()));
    }

    if let Some(line_height) = req.line_height {
        sql_parts.push("line_height = ?".to_string());
        params.push(("line_height".to_string(), line_height.to_string()));
    }

    if let Some(letter_spacing) = req.letter_spacing {
        sql_parts.push("letter_spacing = ?".to_string());
        params.push(("letter_spacing".to_string(), letter_spacing.to_string()));
    }

    if let Some(paragraph_spacing) = req.paragraph_spacing {
        sql_parts.push("paragraph_spacing = ?".to_string());
        params.push((
            "paragraph_spacing".to_string(),
            paragraph_spacing.to_string(),
        ));
    }

    if let Some(reading_width) = req.reading_width {
        sql_parts.push("reading_width = ?".to_string());
        params.push(("reading_width".to_string(), reading_width.to_string()));
    }

    if let Some(text_indent) = req.text_indent {
        sql_parts.push("text_indent = ?".to_string());
        params.push(("text_indent".to_string(), text_indent.to_string()));
    }

    if let Some(simplified_chinese) = req.simplified_chinese {
        sql_parts.push("simplified_chinese = ?".to_string());
        params.push((
            "simplified_chinese".to_string(),
            simplified_chinese.to_string(),
        ));
    }

    // 如果没有需要更新的字段，直接返回成功
    if sql_parts.is_empty() {
        return Ok(Json(ApiResponse::<()>::message("无更新内容")));
    }

    // 构建SQL语句
    let sql = format!(
        "UPDATE reading_settings SET {} WHERE user_id = ?",
        sql_parts.join(", ")
    );

    // 执行更新
    let mut query = sqlx::query(&sql);
    for (_, value) in params {
        query = query.bind(value);
    }
    query = query.bind(auth.user_id);

    query.execute(&pool).await?;

    Ok(Json(ApiResponse::<()>::message("更新成功")))
}

// 处理心跳包
async fn process_heartbeat(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Json(req): Json<HeartbeatRequest>,
) -> Result<Json<ApiResponse<HeartbeatResponse>>, AppError> {
    // 检查书籍是否存在
    let book = sqlx::query!(
        "SELECT user_id, is_public FROM books WHERE id = ?",
        req.book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::NotFound("书籍不存在".to_string()))?;

    // 检查权限
    if book.user_id != auth.user_id && !book.is_public {
        return Err(AppError::Forbidden("无权访问该书籍".to_string()));
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
    let is_same_device = last_device_id.as_deref() == Some(current_device_id.as_str());

    // 如果设备不同，返回服务器保存的进度
    if !is_same_device {
        return Ok(Json(ApiResponse::success(HeartbeatResponse {
            synced: false,
            position: progress.position,
            reading_time: progress.reading_time,
        })));
    }

    // 计算阅读时间增量
    let mut reading_time_increment = 0;

    if let Some(last_time) = last_read_at {
        let duration = now.signed_duration_since(last_time.parse::<DateTime<Utc>>().unwrap());
        let seconds = duration.num_seconds();

        // 如果时间间隔在合理范围内（例如，少于30秒），计为阅读时间
        if seconds > 0 && seconds < 30 {
            reading_time_increment = seconds;
        }
    }

    // 更新阅读进度
    let new_reading_time = progress.reading_time + reading_time_increment;

    sqlx::query!(
        "UPDATE reading_progress 
         SET position = ?, reading_time = ?, last_read_at = ?, last_device_id = ? 
         WHERE user_id = ? AND book_id = ?",
        req.position,
        new_reading_time,
        now,
        current_device_id,
        auth.user_id,
        req.book_id
    )
    .execute(&pool)
    .await?;

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

    // 返回同步状态和最新阅读时间
    Ok(Json(ApiResponse::success(HeartbeatResponse {
        synced: true,
        position: req.position,
        reading_time: new_reading_time,
    })))
}
