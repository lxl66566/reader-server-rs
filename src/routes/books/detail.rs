use std::path::PathBuf;

use axum::{
    Json,
    extract::{Path, State},
};
use sqlx::{Pool, Sqlite};

use super::helpers::apply_book_field_updates;
use crate::{
    access::ensure_book_access,
    auth::AuthUser,
    error::{ApiResponse, AppError},
    limits::BOOK_DESCRIPTION_MAX_CHARS,
    models::{
        Book, BookDetailResponse, BookFormat, ChapterResponse, PatternUpdate, UpdateBookRequest,
    },
    parser::extract_chapters,
};

// 获取书籍详情
pub(super) async fn get_book_detail(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
) -> Result<Json<ApiResponse<BookDetailResponse>>, AppError> {
    // 查询书籍信息（已移入回收站的书按不存在处理）
    let book = sqlx::query_as::<_, Book>("SELECT * FROM books WHERE id = ? AND deleted_at IS NULL")
        .bind(book_id)
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| AppError::BookNotFound)?;

    // 检查权限（所有者或公开书）
    ensure_book_access(auth.user_id, book.user_id, book.is_public)?;

    // 查询章节信息
    let chapters = sqlx::query!(
        "SELECT id, title, position FROM chapters WHERE book_id = ? ORDER BY position",
        book_id
    )
    .fetch_all(&pool)
    .await?;

    // 查询阅读进度
    let progress = sqlx::query!(
        r#"SELECT locator, percent, reading_time, last_read_at FROM reading_progress 
         WHERE user_id = ? AND book_id = ?"#,
        auth.user_id,
        book_id
    )
    .fetch_optional(&pool)
    .await?;

    // 如果没有阅读进度，创建一个
    let (locator, percent, reading_time, last_read_at) = if let Some(p) = progress {
        (Some(p.locator), p.percent, p.reading_time, p.last_read_at)
    } else {
        // 如果是公开书籍，为当前用户创建进度记录
        // INSERT OR IGNORE：并发请求会同时进入此分支，裸 INSERT 会让后到的请求撞
        // UNIQUE(user_id, book_id) 变 500，忽略冲突即可（进度行内容一致）
        if book.user_id != auth.user_id {
            sqlx::query!(
                "INSERT OR IGNORE INTO reading_progress (user_id, book_id) VALUES (?, ?)",
                auth.user_id,
                book_id
            )
            .execute(&pool)
            .await?;
        }
        (None, 0.0, 0, None)
    };

    // 构建章节响应
    let chapter_responses: Vec<ChapterResponse> = chapters
        .into_iter()
        .map(|chapter| ChapterResponse {
            chapter_id: chapter.id,
            title: chapter.title,
            position: chapter.position,
        })
        .collect();

    // 构建书籍详情响应
    let response = BookDetailResponse {
        book_id: book.id,
        title: book.title,
        author: book.author,
        is_public: book.is_public,
        format: book.format,
        created_at: book.created_at,
        last_read_at,
        locator,
        percent,
        reading_time,
        total_chars: book.total_chars,
        total_pages: book.total_pages,
        description: book.description,
        is_owner: book.user_id == auth.user_id,
        chapter_pattern: book.chapter_pattern,
        chapters: chapter_responses,
    };

    Ok(Json(ApiResponse::success(response)))
}

// 更新书籍信息
pub(super) async fn update_book(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Json(req): Json<UpdateBookRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // 检查书籍是否存在并属于当前用户（回收站中的书不可编辑）
    let book = sqlx::query!(
        r#"SELECT user_id, format AS "format!: BookFormat", file_path, chapter_pattern FROM books WHERE id = ? AND deleted_at IS NULL"#,
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::BookNotFound)?;

    // 验证权限
    if book.user_id != auth.user_id {
        return Err(AppError::BookModifyDenied);
    }

    // 章节 regex 仅对 txt 有意义：非空值拒绝；null/空串（Reset）对非 txt 无意义，视为未提供
    let pattern_update = match req.chapter_pattern {
        PatternUpdate::Set(_) if book.format != BookFormat::Txt => {
            return Err(AppError::ChapterPatternNotSupported);
        },
        PatternUpdate::Reset if book.format != BookFormat::Txt => PatternUpdate::Unchanged,
        other => other,
    };

    // 与库中现值比较：只有实际变化才重切章节（避免无谓的全文件重提）
    let stored_pattern = book
        .chapter_pattern
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    let pattern_changed = match &pattern_update {
        PatternUpdate::Set(p) => stored_pattern != Some(p.as_str()),
        PatternUpdate::Reset => stored_pattern.is_some(),
        PatternUpdate::Unchanged => false,
    };

    // 简介三态归一：字段缺省 = 不修改（None）；空/纯空白 = 清空（Some(None)）；
    // 非空 = 更新（Some(Some(trimmed))）。超限拒绝（400）
    let description_update = match req.description.as_deref() {
        None => None,
        Some(text) => {
            if text.chars().count() > BOOK_DESCRIPTION_MAX_CHARS {
                return Err(AppError::FieldTooLarge("description"));
            }
            let trimmed = text.trim();
            Some(if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            })
        },
    };

    let has_update = req.title.is_some()
        || req.author.is_some()
        || req.is_public.is_some()
        || req.cover_path.is_some()
        || description_update.is_some()
        || pattern_changed;

    if !has_update {
        return Ok(Json(ApiResponse::<()>::message("No updates")));
    }

    // 变更时从磁盘原文件重提章节；文件可能被外部清理，沿用下载路径的 404 语义
    let new_chapters: Option<Vec<(String, i64)>> = if pattern_changed {
        let path = PathBuf::from(&book.file_path);
        if !path.exists() {
            tracing::error!(book_id, file_path = %book.file_path, "book file missing on disk");
            return Err(AppError::BookNotFound);
        }
        let pattern = match &pattern_update {
            PatternUpdate::Set(p) => Some(p.clone()),
            _ => None,
        };
        // 读全文 + 逐行正则切分为 CPU 密集操作，移入阻塞线程池（参照 user_data.rs）；
        // 上传时已保证 UTF-8，此处失败即文件被外部篡改，沿用 Io 错误语义
        Some(
            tokio::task::spawn_blocking(move || -> Result<Vec<(String, i64)>, AppError> {
                let text = std::fs::read_to_string(&path).map_err(AppError::Io)?;
                extract_chapters(&text, pattern.as_deref())
            })
            .await
            .map_err(|e| AppError::Internal(format!("chapter re-split task failed: {e}")))??,
        )
    } else {
        None
    };

    // 元数据更新与章节重切放同一事务，避免出现“规则已更新但章节未变”的中间态
    let mut tx = pool.begin().await?;

    apply_book_field_updates(
        &mut tx,
        book_id,
        req.title.as_deref(),
        req.author.as_deref(),
        req.is_public,
        req.cover_path.as_deref(),
    )
    .await?;

    // 简介独立更新：apply_book_field_updates 与管理员编辑路径共用，签名不扩散；
    // 绑定 Option<String>，None 即清空为 NULL
    if let Some(value) = &description_update {
        sqlx::query!(
            "UPDATE books SET description = ? WHERE id = ?",
            value,
            book_id
        )
        .execute(&mut *tx)
        .await?;
    }

    if pattern_changed {
        // 恢复默认时写 NULL
        let stored: Option<String> = match &pattern_update {
            PatternUpdate::Set(p) => Some(p.clone()),
            _ => None,
        };
        sqlx::query!(
            "UPDATE books SET chapter_pattern = ? WHERE id = ?",
            stored,
            book_id
        )
        .execute(&mut *tx)
        .await?;
    }

    if let Some(chapters) = new_chapters {
        sqlx::query!("DELETE FROM chapters WHERE book_id = ?", book_id)
            .execute(&mut *tx)
            .await?;
        if !chapters.is_empty() {
            let mut chapter_qb =
                sqlx::QueryBuilder::new("INSERT INTO chapters (book_id, title, position) ");
            chapter_qb.push_values(chapters.iter(), |mut b, (title, position)| {
                b.push_bind(book_id)
                    .push_bind(title.clone())
                    .push_bind(*position);
            });
            chapter_qb.build().execute(&mut *tx).await?;
        }
    }

    tx.commit().await?;

    Ok(Json(ApiResponse::<()>::message("Update successful")))
}
