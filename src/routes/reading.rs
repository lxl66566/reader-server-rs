use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde::Deserialize;
use sqlx::{Pool, Sqlite};

use crate::{
    access::check_book_access,
    auth::AuthUser,
    error::{ApiResponse, AppError},
    limits::DAILY_STATS_MAX_ENTRIES,
    models::{
        DailyBookReadingStat, DailyReadingStatsResponse, DailyStatEntriesResponse, DailyStatEntry,
        ReadingSettings, ReadingStatsResponse, SetReadingTimeRequest, SetReadingTimeResponse,
        UpdateDailyStatsRequest, UpdateReadingSettingsRequest,
    },
    utils::make_etag,
};

/// 默认时区（无法解析用户时区或用户未设置时使用）
const DEFAULT_TZ: chrono_tz::Tz = chrono_tz::UTC;

// —— 阅读设置取值范围 ——
// 范围需兼容两处既有取值：前端设置面板控件（em 单位滑块，如阅读宽度 20-100）
// 与历史 schema 默认值（用户原样回写默认设置也必须通过校验）。故此处取
// 覆盖两者的宽范围，仅用于拒绝负数、零与明显异常的数值。
/// 字号（px）
const FONT_SIZE_RANGE: std::ops::RangeInclusive<i64> = 8..=72;
/// 行距倍数
const LINE_HEIGHT_RANGE: std::ops::RangeInclusive<f64> = 1.0..=3.0;
/// 字间距（px）
const LETTER_SPACING_RANGE: std::ops::RangeInclusive<f64> = 0.0..=20.0;
/// 段间距（em）
const PARAGRAPH_SPACING_RANGE: std::ops::RangeInclusive<f64> = 0.0..=10.0;
/// 阅读区域宽度（前端按 em 使用）
const READING_WIDTH_RANGE: std::ops::RangeInclusive<i64> = 10..=2000;
/// 段落首行缩进（字符数）
const TEXT_INDENT_RANGE: std::ops::RangeInclusive<f64> = 0.0..=8.0;

/// 校验 hex 颜色格式：`#RGB` 或 `#RRGGBB`（大小写不敏感）
fn is_hex_color(s: &str) -> bool {
    let Some(hex) = s.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 6) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 解析时区字符串；失败时回退到 UTC
fn parse_tz(s: &str) -> chrono_tz::Tz {
    s.parse().unwrap_or(DEFAULT_TZ)
}

/// 返回用户本地"现在"；未设置时区按 UTC，
/// DB 查询失败记录告警后同样按 UTC 回退（不让统计接口因此 500）
async fn user_local_now(pool: &Pool<Sqlite>, user_id: i64) -> DateTime<chrono_tz::Tz> {
    let tz_str = match sqlx::query_scalar::<_, Option<String>>(
        "SELECT timezone FROM reading_settings WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    {
        // Ok(None)：无设置行；Ok(Some(None))：行存在但未设置时区
        Ok(row) => row.flatten(),
        Err(e) => {
            tracing::warn!(user_id, error = %e, "query user timezone failed, fallback to UTC");
            None
        },
    };
    let tz = tz_str.as_deref().map_or(DEFAULT_TZ, parse_tz);
    Utc::now().with_timezone(&tz)
}

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
        // WebSocket 入口：替代旧的每秒轮询心跳接口
        .route("/ws", get(crate::ws::handler::ws_handler))
        .route("/progress/{book_id}", put(set_book_reading_time))
        .route("/stats", get(get_reading_stats))
        .route("/stats/daily", get(get_daily_reading_stats))
        .route(
            "/stats/daily/{book_id}",
            get(get_book_daily_stats).put(update_book_daily_stats),
        )
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
        // 创建默认设置后回读该行：schema 默认值是唯一事实来源，
        // 避免此处再硬编码一份默认值、与 schema 漂移后首响应用旧值
        sqlx::query!(
            "INSERT OR IGNORE INTO reading_settings (user_id) VALUES (?)",
            auth.user_id
        )
        .execute(&pool)
        .await?;
        sqlx::query_as::<_, ReadingSettings>("SELECT * FROM reading_settings WHERE user_id = ?")
            .bind(auth.user_id)
            .fetch_one(&pool)
            .await?
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
    // 确保设置行存在。INSERT OR IGNORE：并发首更会同时走到这里，
    // 裸 INSERT 会撞 UNIQUE(user_id) 变 500；IGNORE 命中（已被其他请求创建）
    // 时下方 UPDATE 兜底生效，无需关心本请求是否真正建了行
    sqlx::query!(
        "INSERT OR IGNORE INTO reading_settings (user_id) VALUES (?)",
        auth.user_id
    )
    .execute(&pool)
    .await?;

    // 构建动态更新 SQL（使用 separated 自动处理逗号分隔）
    let mut query_builder: sqlx::QueryBuilder<Sqlite> =
        sqlx::QueryBuilder::new("UPDATE reading_settings SET ");
    let mut sep = query_builder.separated(", ");

    let mut has_update = false;
    if let Some(v) = req.font_size {
        if !FONT_SIZE_RANGE.contains(&v) {
            return Err(AppError::SettingOutOfRange("font_size"));
        }
        sep.push("font_size = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.background_color {
        if !is_hex_color(&v) {
            return Err(AppError::InvalidColor);
        }
        sep.push("background_color = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.text_color {
        if !is_hex_color(&v) {
            return Err(AppError::InvalidColor);
        }
        sep.push("text_color = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.line_height {
        if !LINE_HEIGHT_RANGE.contains(&v) {
            return Err(AppError::SettingOutOfRange("line_height"));
        }
        sep.push("line_height = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.letter_spacing {
        if !LETTER_SPACING_RANGE.contains(&v) {
            return Err(AppError::SettingOutOfRange("letter_spacing"));
        }
        sep.push("letter_spacing = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.paragraph_spacing {
        if !PARAGRAPH_SPACING_RANGE.contains(&v) {
            return Err(AppError::SettingOutOfRange("paragraph_spacing"));
        }
        sep.push("paragraph_spacing = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.reading_width {
        if !READING_WIDTH_RANGE.contains(&v) {
            return Err(AppError::SettingOutOfRange("reading_width"));
        }
        sep.push("reading_width = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.text_indent {
        if !TEXT_INDENT_RANGE.contains(&v) {
            return Err(AppError::SettingOutOfRange("text_indent"));
        }
        sep.push("text_indent = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.chinese_conversion {
        sep.push("chinese_conversion = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.timezone {
        // 校验时区名是否合法（必须能解析为 IANA 时区）
        if v.parse::<chrono_tz::Tz>().is_err() {
            return Err(AppError::InvalidTimezone);
        }
        sep.push("timezone = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.font_family {
        // 限制长度，避免滥用；具体字体解析交给前端
        let trimmed = v.trim();
        if trimmed.is_empty() || trimmed.chars().count() > 128 {
            return Err(AppError::InvalidFontFamily);
        }
        sep.push("font_family = ")
            .push_bind_unseparated(trimmed.to_string());
        has_update = true;
    }
    if let Some(v) = req.theme {
        sep.push("theme = ").push_bind_unseparated(v);
        has_update = true;
    }
    if let Some(v) = req.pagination_mode {
        sep.push("pagination_mode = ").push_bind_unseparated(v);
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

/// 心跳处理结果：供 HTTP 与 WS 复用
#[derive(Debug, Clone)]
pub struct HeartbeatResult {
    pub synced: bool,
    pub locator: String,
    pub percent: f64,
    pub reading_time: i64,
    /// 本次心跳是否实际更新了服务端进度
    /// （WS 据此决定是否向其他设备广播进度更新）
    pub updated: bool,
}

/// 心跳逻辑所需的 reading_progress 当前行
struct ProgressRow {
    locator: String,
    percent: f64,
    reading_time: i64,
    last_read_at: Option<String>,
    last_device_id: Option<String>,
}

/// 读取进度行；不存在返回 None
async fn fetch_progress(
    pool: &Pool<Sqlite>,
    user_id: i64,
    book_id: i64,
) -> Result<Option<ProgressRow>, AppError> {
    Ok(sqlx::query_as!(
        ProgressRow,
        "SELECT locator, percent, reading_time, last_read_at, last_device_id 
             FROM reading_progress 
             WHERE user_id = ? AND book_id = ?",
        user_id,
        book_id
    )
    .fetch_optional(pool)
    .await?)
}

/// 处理心跳的核心逻辑
///
/// 设计要点：
/// - 首次心跳会创建进度记录；并发首心跳败者（INSERT 被忽略）走让位语义
/// - 若 `last_device_id` 与本次不同，返回服务器进度（让新设备同步），不更新进度/时长， 但会将
///   `last_device_id` 让位给本设备，使其下一次心跳即可接管写入
/// - 同设备正常更新进度，累加阅读时间（与上次间隔 < 30s 才计入）
/// - 使用乐观锁：若并发心跳抢先更新，本次视为未更新但返回最新进度
///
/// `locator` 是与书籍格式相关的位置字符串（txt=字符偏移、epub=EpubCFI、
/// pdf=页码）； `percent` 是前端计算的精确进度（0-1），后端不做语义解析，
/// 只持久化+广播。
pub async fn process_heartbeat_logic(
    pool: &Pool<Sqlite>,
    user_id: i64,
    book_id: i64,
    locator: &str,
    percent: f64,
    device_id: &str,
) -> Result<HeartbeatResult, AppError> {
    // 校验 locator 非空（避免空字符串污染进度）
    if locator.is_empty() {
        return Err(AppError::EmptyLocator);
    }

    // 校验 percent 范围（服务端不解析 locator，但需要可靠的进度展示）
    if !(0.0..=1.0).contains(&percent) {
        return Err(AppError::InvalidPercent);
    }

    // 检查书籍存在性与读权限（所有者或公开书）
    check_book_access(pool, user_id, book_id).await?;

    let now = Utc::now();
    let now_str = now.to_rfc3339_opts(SecondsFormat::Millis, true);

    // 如果没有阅读进度记录，创建一个
    let progress = if let Some(p) = fetch_progress(pool, user_id, book_id).await? {
        p
    } else {
        // INSERT OR IGNORE：并发首心跳会同时进入此分支，裸 INSERT 会让后到的请求撞
        // UNIQUE(user_id, book_id) 变 500
        let res = sqlx::query!(
            "INSERT OR IGNORE INTO reading_progress 
             (user_id, book_id, locator, percent, last_read_at, last_device_id) 
             VALUES (?, ?, ?, ?, ?, ?)",
            user_id,
            book_id,
            locator,
            percent,
            now_str,
            device_id
        )
        .execute(pool)
        .await?;

        if res.rows_affected() == 1 {
            return Ok(HeartbeatResult {
                synced: true,
                locator: locator.to_string(),
                percent,
                reading_time: 0,
                updated: true,
            });
        }
        // 并发首心跳败者：行已被抢插（rows_affected == 0），重查现值
        // 走下方统一流程——设备不同则进让位分支，返回 synced=false + DB 现值，
        // 避免谎称已保存并广播 DB 中不存在的进度
        fetch_progress(pool, user_id, book_id)
            .await?
            .ok_or(AppError::Internal("reading_progress row vanished".into()))?
    };
    let last_device_id = progress.last_device_id;
    let last_read_at = progress.last_read_at;

    // 检查设备ID是否相同
    let is_same_device = last_device_id.as_deref().is_none_or(|id| id == device_id);

    // 如果设备不同，返回服务器保存的进度（让新设备同步），不更新进度/时长；
    // 但将 last_device_id 让位给本设备——否则旧设备停止心跳后新设备永远无法接管，
    // 造成进度与阅读时长永不落库（多设备接管死锁）
    if !is_same_device {
        sqlx::query!(
            "UPDATE reading_progress SET last_device_id = ? WHERE user_id = ? AND book_id = ?",
            device_id,
            user_id,
            book_id
        )
        .execute(pool)
        .await?;

        return Ok(HeartbeatResult {
            synced: false,
            locator: progress.locator,
            percent: progress.percent,
            reading_time: progress.reading_time,
            updated: false,
        });
    }

    // 计算阅读时间增量；解析失败时跳过（不累加），避免 panic。
    // 时钟回拨防护：seconds <= 0（now 早于上次记录）时丢弃该间隔，
    // 影响上界为一次心跳间隔（<30s），恢复后从新的 last_read_at 起算
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

    // 当日统计日期需在事务开始前取好：事务会占用池中连接，
    // 单连接池（测试）下事务内再查 reading_settings 会死锁
    let today = if reading_time_increment > 0 {
        // 日期按用户本地时区归算，避免 UTC+8 凌晨被记到前一天
        Some(
            user_local_now(pool, user_id)
                .await
                .format("%Y-%m-%d")
                .to_string(),
        )
    } else {
        None
    };

    // 三条写语句（progress / users / daily_reading_stats）包单事务，
    // 避免部分成功导致 reading_time 与用户/每日统计不一致
    let mut tx = pool.begin().await?;

    // 使用乐观锁更新阅读进度：WHERE 条件包含 last_read_at 原始值，
    // 若在此期间被其他并发心跳修改，rows_affected == 0。
    // 使用 reading_time = reading_time + ? 确保原子累加。
    let mut qb = sqlx::QueryBuilder::new("UPDATE reading_progress SET locator = ");
    qb.push_bind(locator);
    qb.push(", percent = ");
    qb.push_bind(percent);
    qb.push(", reading_time = reading_time + ");
    qb.push_bind(reading_time_increment);
    qb.push(", last_read_at = ");
    qb.push_bind(&now_str);
    qb.push(", last_device_id = ");
    qb.push_bind(device_id);
    qb.push(" WHERE user_id = ");
    qb.push_bind(user_id);
    qb.push(" AND book_id = ");
    qb.push_bind(book_id);
    qb.push(" AND ");
    match &last_read_at {
        Some(t) => {
            qb.push("last_read_at = ");
            qb.push_bind(t);
        },
        None => {
            qb.push("last_read_at IS NULL");
        },
    }

    let result = qb.build().execute(&mut *tx).await?;

    // 乐观锁竞争失败：并发请求已抢先更新，返回当前最新进度（设备相同 →
    // synced=true，但未广播）。本事务尚无任何写入，直接回滚
    if result.rows_affected() == 0 {
        tx.rollback().await?;
        let current = sqlx::query!(
            "SELECT locator, percent, reading_time FROM reading_progress WHERE user_id = ? AND \
             book_id = ?",
            user_id,
            book_id
        )
        .fetch_one(pool)
        .await?;

        return Ok(HeartbeatResult {
            synced: true,
            locator: current.locator,
            percent: current.percent,
            reading_time: current.reading_time,
            updated: false,
        });
    }

    if reading_time_increment > 0
        && let Some(today) = today
    {
        // 更新用户总阅读时间
        sqlx::query!(
            "UPDATE users 
             SET total_reading_time = total_reading_time + ? 
             WHERE id = ?",
            reading_time_increment,
            user_id
        )
        .execute(&mut *tx)
        .await?;

        // 同步累加当日阅读统计（按书归档）；INSERT ON CONFLICT 处理 upsert
        sqlx::query!(
            "INSERT INTO daily_reading_stats (user_id, book_id, date, reading_seconds) VALUES (?, \
             ?, ?, ?) ON CONFLICT (user_id, book_id, date) DO UPDATE SET reading_seconds = \
             reading_seconds + excluded.reading_seconds",
            user_id,
            book_id,
            today,
            reading_time_increment,
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    let new_reading_time = progress.reading_time + reading_time_increment;

    Ok(HeartbeatResult {
        synced: true,
        locator: locator.to_string(),
        percent,
        reading_time: new_reading_time,
        updated: true,
    })
}

/// 手动编辑阅读时长的上限（秒）：3650 天，防止误输入导致统计失真。
/// 与 AppError::ReadingTimeOutOfRange 的文案保持同步
pub const MAX_READING_TIME_SECS: i64 = 3650 * 24 * 60 * 60;

/// 手动设置 (user, book) 的累计阅读时长（秒），供用户自助与管理员修正共用。
///
/// 设计要点：
/// - 差值原子累加到 `users.total_reading_time`，读旧值与两次写入包单事务，
///   避免无事务读改写丢失并发心跳的增量
/// - 每日统计 `daily_reading_stats` 按天聚合、无法归属历史天数，保持不动
/// - 无进度行且新值为 0 时无写入，不制造空进度行
pub async fn set_reading_time(
    pool: &Pool<Sqlite>,
    user_id: i64,
    book_id: i64,
    new_secs: i64,
) -> Result<(), AppError> {
    if !(0..=MAX_READING_TIME_SECS).contains(&new_secs) {
        return Err(AppError::ReadingTimeOutOfRange);
    }

    let mut tx = pool.begin().await?;

    let old: Option<i64> = sqlx::query_scalar!(
        "SELECT reading_time FROM reading_progress WHERE user_id = ? AND book_id = ?",
        user_id,
        book_id
    )
    .fetch_optional(&mut *tx)
    .await?;

    match old {
        Some(old) => {
            sqlx::query!(
                "UPDATE reading_progress SET reading_time = ? WHERE user_id = ? AND book_id = ?",
                new_secs,
                user_id,
                book_id
            )
            .execute(&mut *tx)
            .await?;
            // 差值累加：new_secs - old 可为负，同步回退总时长
            sqlx::query!(
                "UPDATE users SET total_reading_time = total_reading_time + ? WHERE id = ?",
                new_secs - old,
                user_id
            )
            .execute(&mut *tx)
            .await?;
        },
        // 无进度行且新值为 0：等价于现状，无需写入
        None if new_secs == 0 => {},
        None => {
            // 无进度行：按 0 起算建行，locator/percent/last_read_at 保持默认
            sqlx::query!(
                "INSERT INTO reading_progress (user_id, book_id, reading_time) VALUES (?, ?, ?)",
                user_id,
                book_id,
                new_secs
            )
            .execute(&mut *tx)
            .await?;
            sqlx::query!(
                "UPDATE users SET total_reading_time = total_reading_time + ? WHERE id = ?",
                new_secs,
                user_id
            )
            .execute(&mut *tx)
            .await?;
        },
    }

    tx.commit().await?;
    Ok(())
}

/// 手动设置当前用户对某本书的累计阅读时长
async fn set_book_reading_time(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Json(req): Json<SetReadingTimeRequest>,
) -> Result<Json<ApiResponse<SetReadingTimeResponse>>, AppError> {
    // 书籍存在性与读权限（所有者或公开书），与其他阅读接口一致
    check_book_access(&pool, auth.user_id, book_id).await?;

    set_reading_time(&pool, auth.user_id, book_id, req.reading_time).await?;

    Ok(Json(ApiResponse::success(SetReadingTimeResponse {
        reading_time: req.reading_time,
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

// 阅读统计汇总：总时长、连续天数
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

    // 日期边界按用户本地时区归算（避免 UTC 与本地日期错位导致连续天数错乱）
    let local_now = user_local_now(&pool, auth.user_id).await;
    let today = local_now.format("%Y-%m-%d").to_string();
    let yesterday = (local_now - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();

    // 取所有日期用于计算连续天数；未来日期不入序列（每日补录入口允许任意日期，
    // 若混入会把 current_streak 的首日期判成中断）
    let all_dates: Vec<String> = sqlx::query_scalar(
        "SELECT date FROM daily_reading_stats WHERE user_id = ? AND date <= ? ORDER BY date DESC",
    )
    .bind(auth.user_id)
    .bind(&today)
    .fetch_all(&pool)
    .await?;

    // 当前连续天数：从今天或昨天开始往前数
    // longest_streak：扫描整个日期序列找最长连续段
    let (current_streak, longest_streak) = compute_streaks(&all_dates, &today, &yesterday);

    Ok(Json(ApiResponse::success(ReadingStatsResponse {
        total_reading_seconds: total,
        current_streak_days: current_streak,
        longest_streak_days: longest_streak,
    })))
}

// 区间明细查询参数
#[derive(Debug, Deserialize)]
struct DailyStatsQuery {
    start: String,
    end: String,
}

/// 区间上限：年视图需要完整 12 个月（366/371 天），放宽到 372 天
const MAX_DAILY_RANGE_DAYS: i64 = 372;

// 阅读时长区间明细：逐日、按书拆分（前端自行聚合为周/月/年分桶）
async fn get_daily_reading_stats(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Query(q): Query<DailyStatsQuery>,
) -> Result<Json<ApiResponse<DailyReadingStatsResponse>>, AppError> {
    let start = parse_date(&q.start).ok_or(AppError::InvalidDateRange)?;
    let end = parse_date(&q.end).ok_or(AppError::InvalidDateRange)?;
    if start > end || (end - start).num_days() > MAX_DAILY_RANGE_DAYS {
        return Err(AppError::InvalidDateRange);
    }

    // book 级联删除，JOIN 必命中；回收站中的书不出现（恢复后重新可见）
    let days: Vec<DailyBookReadingStat> = sqlx::query_as::<_, DailyBookReadingStat>(
        "SELECT s.date, s.book_id, b.title, s.reading_seconds FROM daily_reading_stats s JOIN \
         books b ON b.id = s.book_id AND b.deleted_at IS NULL WHERE s.user_id = ? AND s.date >= ? \
         AND s.date <= ? ORDER BY s.date ASC, s.reading_seconds DESC",
    )
    .bind(auth.user_id)
    .bind(start.format("%Y-%m-%d").to_string())
    .bind(end.format("%Y-%m-%d").to_string())
    .fetch_all(&pool)
    .await?;

    Ok(Json(ApiResponse::success(DailyReadingStatsResponse {
        days,
    })))
}

// 获取当前用户对某本书的全部每日阅读时长记录（date 升序）
async fn get_book_daily_stats(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
) -> Result<Json<ApiResponse<DailyStatEntriesResponse>>, AppError> {
    // 书籍存在性与读权限（所有者或公开书），与其他阅读接口一致
    check_book_access(&pool, auth.user_id, book_id).await?;

    let entries = sqlx::query_as::<_, DailyStatEntry>(
        "SELECT date, reading_seconds FROM daily_reading_stats WHERE user_id = ? AND book_id = ? \
         ORDER BY date ASC",
    )
    .bind(auth.user_id)
    .bind(book_id)
    .fetch_all(&pool)
    .await?;

    Ok(Json(ApiResponse::success(DailyStatEntriesResponse {
        entries,
    })))
}

/// 校验并规范化每日条目：日期合法且不重复、秒数在 0..=MAX；
/// 0 秒条目剔除（与“无记录”等价），结果按日期升序
fn normalize_daily_entries(
    entries: Vec<DailyStatEntry>,
) -> Result<Vec<(NaiveDate, i64)>, AppError> {
    let mut seen = std::collections::HashSet::with_capacity(entries.len());
    let mut normalized = Vec::with_capacity(entries.len());
    for entry in entries {
        let date = parse_date(&entry.date).ok_or(AppError::InvalidDailyStatDate)?;
        // 重复日期入库会撞 UNIQUE(user_id, book_id, date)，须在此拒绝
        if !seen.insert(date) {
            return Err(AppError::DuplicateDailyStatDate);
        }
        if !(0..=MAX_READING_TIME_SECS).contains(&entry.reading_seconds) {
            return Err(AppError::ReadingTimeOutOfRange);
        }
        if entry.reading_seconds > 0 {
            normalized.push((date, entry.reading_seconds));
        }
    }
    normalized.sort_by_key(|(date, _)| *date);
    Ok(normalized)
}

/// 全量替换当前用户对某本书的每日阅读时长记录。
///
/// 设计要点：
/// - 差值同步：`delta = sum(new) - sum(old)`，在同一事务内调整
///   `reading_progress.reading_time`（建行/原子差值，与 [`set_reading_time`] 同语义）与
///   `users.total_reading_time`；两者结果均钳制 >= 0（进度行另钳制
///   到上限），因钳制导致的总量与每日之和不一致属预期
/// - 总时长可能早于每日统计功能存在（useTime != sum(daily) 同语义），
///   因此只按差值同步而非改写为每日之和
async fn update_book_daily_stats(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Json(req): Json<UpdateDailyStatsRequest>,
) -> Result<Json<ApiResponse<DailyStatEntriesResponse>>, AppError> {
    // 书籍存在性与读权限（所有者或公开书），与其他阅读接口一致
    check_book_access(&pool, auth.user_id, book_id).await?;

    // 条目数量上限（见 crate::limits）：防构造性大请求放大事务与批量 INSERT
    if req.entries.len() > DAILY_STATS_MAX_ENTRIES {
        return Err(AppError::TooManyDailyStatsEntries);
    }

    let new_entries = normalize_daily_entries(req.entries)?;

    let mut tx = pool.begin().await?;

    // 旧每日记录总和，供差值同步
    let old_sum: i64 = sqlx::query_scalar!(
        "SELECT COALESCE(SUM(reading_seconds), 0) FROM daily_reading_stats WHERE user_id = ? AND \
         book_id = ?",
        auth.user_id,
        book_id
    )
    .fetch_one(&mut *tx)
    .await?;

    // 全量替换
    sqlx::query!(
        "DELETE FROM daily_reading_stats WHERE user_id = ? AND book_id = ?",
        auth.user_id,
        book_id
    )
    .execute(&mut *tx)
    .await?;
    if !new_entries.is_empty() {
        // 批量插入：条目数受 DAILY_STATS_MAX_ENTRIES 钳制，
        // 绑定参数总量远低于 SQLite 变量上限
        let mut qb = sqlx::QueryBuilder::new(
            "INSERT INTO daily_reading_stats (user_id, book_id, date, reading_seconds) ",
        );
        qb.push_values(new_entries.iter(), |mut b, (date, secs)| {
            b.push_bind(auth.user_id)
                .push_bind(book_id)
                .push_bind(date.format("%Y-%m-%d").to_string())
                .push_bind(secs);
        });
        qb.build().execute(&mut *tx).await?;
    }

    let delta: i64 = new_entries.iter().map(|&(_, secs)| secs).sum::<i64>() - old_sum;

    // 差值同步累计时长（钳制到 [0, MAX]）；实际生效差值同步到用户总时长（钳制 >= 0）
    let old_progress: Option<i64> = sqlx::query_scalar!(
        "SELECT reading_time FROM reading_progress WHERE user_id = ? AND book_id = ?",
        auth.user_id,
        book_id
    )
    .fetch_optional(&mut *tx)
    .await?;

    match old_progress {
        Some(old) => {
            let target = (old + delta).clamp(0, MAX_READING_TIME_SECS);
            sqlx::query!(
                "UPDATE reading_progress SET reading_time = ? WHERE user_id = ? AND book_id = ?",
                target,
                auth.user_id,
                book_id
            )
            .execute(&mut *tx)
            .await?;
            sqlx::query!(
                "UPDATE users SET total_reading_time = MAX(0, total_reading_time + ?) WHERE id = ?",
                target - old,
                auth.user_id
            )
            .execute(&mut *tx)
            .await?;
        },
        // 无进度行且目标值为 0：等价于现状，不建空进度行
        None if delta <= 0 => {},
        None => {
            let target = delta.clamp(0, MAX_READING_TIME_SECS);
            sqlx::query!(
                "INSERT INTO reading_progress (user_id, book_id, reading_time) VALUES (?, ?, ?)",
                auth.user_id,
                book_id,
                target
            )
            .execute(&mut *tx)
            .await?;
            sqlx::query!(
                "UPDATE users SET total_reading_time = MAX(0, total_reading_time + ?) WHERE id = ?",
                target,
                auth.user_id
            )
            .execute(&mut *tx)
            .await?;
        },
    }

    tx.commit().await?;

    Ok(Json(ApiResponse::success(DailyStatEntriesResponse {
        entries: new_entries
            .into_iter()
            .map(|(date, secs)| DailyStatEntry {
                date: date.format("%Y-%m-%d").to_string(),
                reading_seconds: secs,
            })
            .collect(),
    })))
}

/// 解析 YYYY-MM-DD 日期字符串（恰好 10 个字符的完整日期）
fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
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
                prev.clone_from(d);
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
            },
            _ => run = 1,
        }
        longest = longest.max(run);
        prev = Some(d.clone());
    }

    (current_streak, longest)
}
