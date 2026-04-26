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

    // 构建更新SQL
    let mut query_builder: sqlx::QueryBuilder<'_, sqlx::Sqlite> =
        sqlx::QueryBuilder::new("UPDATE reading_settings SET ");
    let mut first = true;

    if let Some(font_size) = req.font_size {
        if !first {
            query_builder.push(", ");
        }
        query_builder.push("font_size = ").push_bind(font_size);
        first = false;
    }

    if let Some(background_color) = req.background_color {
        if !first {
            query_builder.push(", ");
        }
        query_builder
            .push("background_color = ")
            .push_bind(background_color);
        first = false;
    }

    if let Some(text_color) = req.text_color {
        if !first {
            query_builder.push(", ");
        }
        query_builder.push("text_color = ").push_bind(text_color);
        first = false;
    }

    if let Some(line_height) = req.line_height {
        if !first {
            query_builder.push(", ");
        }
        query_builder.push("line_height = ").push_bind(line_height);
        first = false;
    }

    if let Some(letter_spacing) = req.letter_spacing {
        if !first {
            query_builder.push(", ");
        }
        query_builder
            .push("letter_spacing = ")
            .push_bind(letter_spacing);
        first = false;
    }

    if let Some(paragraph_spacing) = req.paragraph_spacing {
        if !first {
            query_builder.push(", ");
        }
        query_builder
            .push("paragraph_spacing = ")
            .push_bind(paragraph_spacing);
        first = false;
    }

    if let Some(reading_width) = req.reading_width {
        if !first {
            query_builder.push(", ");
        }
        query_builder
            .push("reading_width = ")
            .push_bind(reading_width);
        first = false;
    }

    if let Some(text_indent) = req.text_indent {
        if !first {
            query_builder.push(", ");
        }
        query_builder.push("text_indent = ").push_bind(text_indent);
        first = false;
    }

    if let Some(simplified_chinese) = req.simplified_chinese {
        if !first {
            query_builder.push(", ");
        }
        query_builder
            .push("simplified_chinese = ")
            .push_bind(simplified_chinese);
        first = false;
    }

    if first {
        return Ok(Json(ApiResponse::<()>::message("无更新内容")));
    }

    query_builder
        .push(" WHERE user_id = ")
        .push_bind(auth.user_id);
    query_builder.build().execute(&pool).await?;

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
