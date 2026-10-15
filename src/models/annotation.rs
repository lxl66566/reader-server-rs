use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ============ 书签 / 批注 ============

/// 批注类型枚举：note=纯笔记；highlight=高亮
/// 序列化对外用字符串（保持 API 稳定可读），DB CHECK 约束同步
#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum AnnotationKind {
    Note,
    Highlight,
}

// 书签模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Bookmark {
    pub id: i64,
    pub user_id: i64,
    pub book_id: i64,
    /// 与 format 相关的位置字符串（与 `reading_progress.locator` 同语义）
    pub locator: String,
    /// 进度（0-1），用于列表展示与排序
    pub percent: f64,
    pub label: Option<String>,
    pub created_at: String,
}

// 创建书签请求
#[derive(Debug, Deserialize)]
pub struct CreateBookmarkRequest {
    pub locator: String,
    /// 进度（0-1），由前端计算
    pub percent: f64,
    pub label: Option<String>,
}

// 批注模型
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Annotation {
    pub id: i64,
    pub user_id: i64,
    pub book_id: i64,
    pub kind: AnnotationKind,
    /// 起始位置（locator），与 format 相关
    pub start_locator: String,
    /// 结束位置（locator），仅 highlight 必填
    pub end_locator: Option<String>,
    /// 起始进度（0-1）
    pub start_percent: f64,
    /// 结束进度（0-1），仅 highlight 有
    pub end_percent: Option<f64>,
    pub excerpt: Option<String>,
    pub content: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// 创建批注请求
#[derive(Debug, Deserialize)]
pub struct CreateAnnotationRequest {
    pub kind: AnnotationKind,
    pub start_locator: String,
    pub end_locator: Option<String>,
    /// 进度（0-1），由前端计算
    pub start_percent: f64,
    pub end_percent: Option<f64>,
    pub excerpt: Option<String>,
    pub content: Option<String>,
}

// 更新批注请求（只允许改 content/excerpt，kind/位置不可改）
#[derive(Debug, Deserialize)]
pub struct UpdateAnnotationRequest {
    pub excerpt: Option<String>,
    pub content: Option<String>,
}

// 书签列表响应
#[derive(Debug, Serialize)]
pub struct ListBookmarksResponse {
    pub bookmarks: Vec<Bookmark>,
}

// 批注列表响应
#[derive(Debug, Serialize)]
pub struct ListAnnotationsResponse {
    pub annotations: Vec<Annotation>,
}
