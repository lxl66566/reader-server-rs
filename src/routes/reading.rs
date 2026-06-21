use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::{Pool, Sqlite};

use crate::{
    auth::AuthUser,
    error::{ApiResponse, AppError},
    models::{
        DailyReadingStat, HeartbeatRequest, HeartbeatResponse, ReadingSettings,
        ReadingStatsResponse, UpdateReadingSettingsRequest,
    },
    utils::make_etag,
};

/// 检查请求头 If-None-Match 是否命中给定 `ETag`
/// 处理 `*` 通配、`W/` 前缀、以及逗号分隔的多个 `ETag`
pub fn etag_matches(if_none_match: &str, etag: &str) -> bool {
    /// 去掉 weak 前缀 W/ 或 w/，便于比较
    fn strip_weak(s: &str) -> &str {
        s.trim_start_matches("W/").trim_start_matches("w/")
    }

    for candidate in if_none_match.split(',') {
        let candidate = candidate.trim();
        if candidate == "*" {
            return true;
        }
        if strip_weak(candidate) == strip_weak(etag) {
            return true;
        }
    }
    false
}

// 阅读路由
pub fn routes() -> Router<crate::app::AppState> {
    Router::new()
        .route(
            "/settings",
            get(get_reading_settings).put(update_reading_settings),
        )
        .route("/heartbeat", post(process_heartbeat))
        .route("/stats", get(get_reading_stats))
}

// 获取阅读设置
async fn get_reading_settings(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
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

    // 序列化 payload，基于其内容生成 ETag；客户端 If-None-Match 命中则返回 304
    let body = serde_json::to_string(&settings).map_err(|e| AppError::Internal(e.to_string()))?;
    let etag = make_etag(&body);

    if let Some(if_none) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && etag_matches(if_none, &etag)
    {
        return Ok(axum::http::StatusCode::NOT_MODIFIED.into_response());
    }

    Ok((
        [(header::ETAG, etag.as_str())],
        Json(ApiResponse::success(settings)),
    )
        .into_response())
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
        sep.push("background_color = ").push_bind_unseparated(v);
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
        sep.push("letter_spacing = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.paragraph_spacing {
        sep.push("paragraph_spacing = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.reading_width {
        sep.push("reading_width = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.text_indent {
        sep.push("text_indent = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.simplified_chinese {
        sep.push("simplified_chinese = ").push_bind_unseparated(v);
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

    // 乐观锁竞争失败：并发请求已抢先更新，返回当前最新进度（设备相同 →
    // synced=true）
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

        // 同步累加当日阅读统计；INSERT ON CONFLICT 处理 upsert
        let today = now.format("%Y-%m-%d").to_string();
        sqlx::query!(
            "INSERT INTO daily_reading_stats (user_id, date, reading_seconds) VALUES (?, ?, ?) \
             ON CONFLICT (user_id, date) DO UPDATE SET reading_seconds = reading_seconds + excluded.reading_seconds",
            auth.user_id,
            today,
            reading_time_increment,
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

/// 计算两个日期字符串（YYYY-MM-DD）之间相隔的天数（later - earlier）
/// 解析失败时返回 None；用于连续阅读天数判断
fn days_between(later: &str, earlier: &str) -> Option<i64> {
    use chrono::NaiveDate;
    let l = NaiveDate::parse_from_str(later, "%Y-%m-%d").ok()?;
    let e = NaiveDate::parse_from_str(earlier, "%Y-%m-%d").ok()?;
    Some((l - e).num_days())
}

// 阅读统计：总时长、连续天数、最近 30 天每日时长
async fn get_reading_stats(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
) -> Result<Json<ApiResponse<ReadingStatsResponse>>, AppError> {
    // 总时长
    let total: i64 =
        sqlx::query_scalar("SELECT COALESCE(total_reading_time, 0) FROM users WHERE id = ?")
            .bind(auth.user_id)
            .fetch_one(&pool)
            .await?;

    // 最近 30 天的每日时长（按日期升序）
    let recent_days: Vec<DailyReadingStat> = sqlx::query_as::<_, DailyReadingStat>(
        "SELECT date, reading_seconds FROM daily_reading_stats \
         WHERE user_id = ? AND date >= date('now', '-29 days') \
         ORDER BY date ASC",
    )
    .bind(auth.user_id)
    .fetch_all(&pool)
    .await?;

    // 取所有日期用于计算连续天数
    let all_dates: Vec<String> = sqlx::query_scalar(
        "SELECT date FROM daily_reading_stats WHERE user_id = ? ORDER BY date DESC",
    )
    .bind(auth.user_id)
    .fetch_all(&pool)
    .await?;

    let today = Utc::now().format("%Y-%m-%d").to_string();
    let yesterday = (Utc::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

    // 当前连续天数：从今天或昨天开始往前数
    // longest_streak：扫描整个日期序列找最长连续段
    let (current_streak, longest_streak) = compute_streaks(&all_dates, &today, &yesterday);

    Ok(Json(ApiResponse::success(ReadingStatsResponse {
        total_reading_seconds: total,
        current_streak_days: current_streak,
        longest_streak_days: longest_streak,
        recent_days,
    })))
}

/// 给定按日期降序排列的日期列表，计算 `current_streak（从今天/昨天起向前数`）
/// 与 `longest_streak（历史最长连续段`）
fn compute_streaks(sorted_desc_dates: &[String], today: &str, yesterday: &str) -> (i64, i64) {
    if sorted_desc_dates.is_empty() {
        return (0, 0);
    }

    // 当前连续：要求第一个日期是今天或昨天，否则视为中断
    let first = sorted_desc_dates[0].as_str();
    let started_today = first == today;
    let started_yesterday = first == yesterday;

    let mut current_streak: i64 = 0;
    if started_today || started_yesterday {
        // 从 first（today/yesterday）向前数连续天数
        let mut prev = first.to_string();
        current_streak = 1;
        for d in &sorted_desc_dates[1..] {
            if days_between(&prev, d) == Some(1) {
                current_streak += 1;
                prev = d.clone();
            } else {
                break;
            }
        }
    }

    // 最长连续：扫描整个序列（按 desc 排序），相邻 diff==1 视为连续
    let mut longest: i64 = 0;
    let mut run: i64 = 0;
    let mut prev: Option<String> = None;
    for d in sorted_desc_dates {
        match &prev {
            Some(p) if days_between(p, d) == Some(1) => {
                run += 1;
            }
            _ => run = 1,
        }
        longest = longest.max(run);
        prev = Some(d.clone());
    }

    (current_streak, longest)
}
