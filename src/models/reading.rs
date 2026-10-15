use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// 阅读主题预设
/// light=浅色；sepia=护眼米黄；dark=深色；system=跟随系统
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum ReadingTheme {
    Light,
    Sepia,
    Dark,
    System,
}

/// 翻页模式
/// scroll=滚动；paginate=翻页
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum PaginationMode {
    Scroll,
    Paginate,
}

/// 简繁转换方向
/// none=不转换；simplified=转简体；traditional=转繁体
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum ChineseConversion {
    None,
    Simplified,
    Traditional,
}

// 阅读设置模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ReadingSettings {
    pub id: i64,
    pub user_id: i64,
    pub font_size: i64,
    pub background_color: String,
    pub text_color: String,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub paragraph_spacing: f64,
    pub reading_width: i64,
    pub text_indent: f64,
    pub chinese_conversion: ChineseConversion,
    /// IANA 时区名（如 `Asia/Shanghai`），用于阅读统计与连续天数归算
    pub timezone: String,
    /// 字体族标识（前端解释），如 `serif`/`sans-serif`/自定义字体名
    pub font_family: String,
    pub theme: ReadingTheme,
    pub pagination_mode: PaginationMode,
}

// 心跳协议已迁移至 ws::protocol，旧的 HTTP 心跳包数据结构已删除

// 手动设置书籍累计阅读时长请求（秒）
#[derive(Debug, Deserialize)]
pub struct SetReadingTimeRequest {
    pub reading_time: i64,
}

// 手动设置阅读时长响应
#[derive(Debug, Serialize)]
pub struct SetReadingTimeResponse {
    pub reading_time: i64,
}

// 更新阅读设置请求
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateReadingSettingsRequest {
    pub font_size: Option<i64>,
    pub background_color: Option<String>,
    pub text_color: Option<String>,
    pub line_height: Option<f64>,
    pub letter_spacing: Option<f64>,
    pub paragraph_spacing: Option<f64>,
    pub reading_width: Option<i64>,
    pub text_indent: Option<f64>,
    pub chinese_conversion: Option<ChineseConversion>,
    /// IANA 时区名；服务端会校验有效性
    pub timezone: Option<String>,
    pub font_family: Option<String>,
    pub theme: Option<ReadingTheme>,
    pub pagination_mode: Option<PaginationMode>,
}

// ============ 阅读统计 ============

// 阅读统计汇总响应
#[derive(Debug, Serialize)]
pub struct ReadingStatsResponse {
    /// 总阅读时长（秒），与 `users.total_reading_time` 一致
    pub total_reading_seconds: i64,
    /// 连续阅读天数（含今天或昨天，按最近一次阅读计算）
    pub current_streak_days: i64,
    /// 历史最长连续阅读天数
    pub longest_streak_days: i64,
}

// 区间内某日某书的阅读时长
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct DailyBookReadingStat {
    pub date: String,
    pub book_id: i64,
    pub title: String,
    pub reading_seconds: i64,
}

// 阅读时长区间明细响应：仅含有记录的日期，按日期升序、单日内按时长降序
#[derive(Debug, Serialize)]
pub struct DailyReadingStatsResponse {
    pub days: Vec<DailyBookReadingStat>,
}

// 单本书每日阅读时长条目（编辑接口的请求与响应共用）
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct DailyStatEntry {
    /// YYYY-MM-DD
    pub date: String,
    pub reading_seconds: i64,
}

// 单本书每日阅读时长响应：date 升序
#[derive(Debug, Serialize)]
pub struct DailyStatEntriesResponse {
    pub entries: Vec<DailyStatEntry>,
}

// 全量替换单本书每日阅读时长请求
#[derive(Debug, Deserialize)]
pub struct UpdateDailyStatsRequest {
    pub entries: Vec<DailyStatEntry>,
}
