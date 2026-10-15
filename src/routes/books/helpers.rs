use std::path::PathBuf;

use axum::http::StatusCode;
use serde::Deserialize;
use sqlx::Sqlite;
use tokio::fs;

use crate::{error::AppError, models::BookFormat};

// 分页查询参数
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

impl PaginationParams {
    /// 归一化分页参数，返回可直接用于 SQL 的 `(limit, offset)`。
    /// page 最小为 1（避免 `(page - 1)` u32 下溢），limit 钳制在 `[1, max_limit]`
    /// （避免无上限拉取全表）；offset 先转 i64 再乘，避免 u32 乘法溢出。
    pub fn normalize(&self, default_limit: u32, max_limit: u32) -> (u32, i64) {
        let page = self.page.unwrap_or(1).max(1);
        let limit = self.limit.unwrap_or(default_limit).clamp(1, max_limit);
        (limit, i64::from(page - 1) * i64::from(limit))
    }
}

/// 映射 multipart 解析错误：超过请求体上限时返回 413，其余按原有错误处理
pub(crate) fn map_multipart_err(
    e: &axum::extract::multipart::MultipartError,
    other: impl FnOnce(String) -> AppError,
) -> AppError {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        AppError::RequestBodyTooLarge
    } else {
        other(e.to_string())
    }
}

/// 标签净化：trim、去空、长度限制（32 字符）、去重（保留首次出现顺序）。
/// set_book_tags 与用户数据导入共用同一规则
pub(crate) fn clean_tags(tags: impl IntoIterator<Item = String>) -> Vec<String> {
    let cleaned: Vec<String> = tags
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.chars().count() <= 32)
        .collect();
    let mut seen = std::collections::HashSet::new();
    cleaned
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

/// 单本书标签数量上限（与 [`clean_tags`] 的单标签 32 字符限制同值）。
/// 对齐 WS 协议侧的输入上限防滥用策略；WS 侧（`src/ws/protocol.rs`）当前
/// 未单独限制 tags 数量，若后续补充对应常量，两处应保持一致
pub(crate) const TAGS_MAX_COUNT: usize = 32;

/// 动态更新书籍元数据字段（仅 SET 提供的列；全为 None 时不执行 SQL）。
/// 用户侧 PUT /books/:id 与管理员侧编辑共用同一更新路径
pub(crate) async fn apply_book_field_updates(
    tx: &mut sqlx::SqliteConnection,
    book_id: i64,
    title: Option<&str>,
    author: Option<&str>,
    is_public: Option<bool>,
    cover_path: Option<&str>,
) -> Result<(), AppError> {
    if title.is_none() && author.is_none() && is_public.is_none() && cover_path.is_none() {
        return Ok(());
    }
    // 构建动态更新 SQL（使用 separated 自动处理逗号分隔）
    let mut query_builder: sqlx::QueryBuilder<Sqlite> =
        sqlx::QueryBuilder::new("UPDATE books SET ");
    let mut sep = query_builder.separated(", ");
    if let Some(title) = title {
        sep.push("title = ")
            .push_bind_unseparated(title.to_string());
    }
    if let Some(author) = author {
        sep.push("author = ")
            .push_bind_unseparated(author.to_string());
    }
    if let Some(is_public) = is_public {
        sep.push("is_public = ").push_bind_unseparated(is_public);
    }
    if let Some(cover_path) = cover_path {
        sep.push("cover_path = ")
            .push_bind_unseparated(cover_path.to_string());
    }
    query_builder.push(" WHERE id = ").push_bind(book_id);
    query_builder.build().execute(tx).await?;
    Ok(())
}

/// 事务内删除书籍 DB 行，返回 `(owner_user_id, 磁盘文件路径)`；外键 ON DELETE
/// CASCADE 级联清理章节/标签关联/书签/批注/阅读进度/每日统计。
/// 磁盘文件由调用方在事务提交成功后删除（孤儿文件比"DB 行存在但文件丢失"更安全）
pub(crate) async fn delete_book_cascade(
    tx: &mut sqlx::SqliteConnection,
    book_id: i64,
) -> Result<(i64, String), AppError> {
    let book = sqlx::query!("SELECT user_id, file_path FROM books WHERE id = ?", book_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::BookNotFound)?;
    sqlx::query!("DELETE FROM books WHERE id = ?", book_id)
        .execute(&mut *tx)
        .await?;
    Ok((book.user_id, book.file_path))
}

/// 删除书籍磁盘文件（best-effort）：文件已不存在则跳过，失败仅记日志不影响响应
pub(crate) async fn remove_book_file(file_path: &str) {
    if PathBuf::from(file_path).exists()
        && let Err(e) = fs::remove_file(file_path).await
    {
        tracing::warn!(error = %e, file_path, "Failed to delete book file");
    }
}

/// 转义 `SQLite` LIKE 的特殊字符（%, _, \），用 ESCAPE '\' 配合
pub(crate) fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    for ch in input.chars() {
        match ch {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(ch);
            },
            _ => out.push(ch),
        }
    }
    out
}

// 列表/搜索共用的行结构（私用 FromRow，避免在闭包里手动取列）
#[derive(sqlx::FromRow)]
pub(super) struct BookListRow {
    pub(super) id: i64,
    pub(super) title: String,
    pub(super) author: Option<String>,
    pub(super) is_public: bool,
    pub(super) created_at: String,
    pub(super) total_chars: i64,
    pub(super) total_pages: i64,
    pub(super) description: Option<String>,
    pub(super) format: BookFormat,
    pub(super) locator: Option<String>,
    pub(super) percent: Option<f64>,
    pub(super) reading_time: Option<i64>,
    pub(super) last_read_at: Option<String>,
}
