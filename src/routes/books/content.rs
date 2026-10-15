use std::path::PathBuf;

use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use sqlx::{Pool, Sqlite};
use tokio::fs;
use tokio_util::io::ReaderStream;

use crate::{
    access::{check_book_access, ensure_book_access},
    auth::AuthUser,
    error::{ApiResponse, AppError},
    models::{BookContentResponse, BookFormat, JumpToChapterResponse},
    routes::reading::etag_matches,
    utils::{make_etag, utf8::Utf8Chunks},
};

// 获取内容查询参数（仅 txt 格式有意义）
#[derive(Debug, Deserialize)]
pub struct ContentParams {
    pub position: i64,
    pub length: Option<i64>,
}

// 跳转章节查询参数
#[derive(Debug, Deserialize)]
pub struct JumpToChapterParams {
    pub chapter_id: i64,
}

// 原文件下载查询参数
#[derive(Debug, Deserialize)]
pub struct FileParams {
    // true 时以 attachment 返回（浏览器保存为文件，文件名取书名）；
    // 缺省 inline（pdf 内联预览，epub.js/pdf.js fetch 渲染不受影响）
    pub download: Option<bool>,
}

/// 下载书籍原始文件
///
/// 用途：
/// - epub / pdf：前端 fetch 后用 epub.js / pdf.js 客户端渲染（缺省 inline）
/// - 下载到本地：`?download=true` 时以 attachment 返回，文件名取书名
/// - txt：可选，前端通常走 /content 分页拉取
///
/// 响应：
/// - 200 + 文件字节流（带 Content-Type、Content-Disposition、ETag）
/// - 客户端可带 If-None-Match 命中 304
///
/// 不实现 HTTP Range：浏览器/CDN 自动处理缓冲；如需 Range 后续可加。
pub(super) async fn download_book_file(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Query(params): Query<FileParams>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let book = sqlx::query!(
        r#"SELECT user_id, title, author, file_path, is_public, content_hash, format AS "format!: BookFormat" FROM books WHERE id = ? AND deleted_at IS NULL"#,
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::BookNotFound)?;

    // 权限：所有者或公开书
    ensure_book_access(auth.user_id, book.user_id, book.is_public)?;

    // 文件可能被外部清理；缺失时返回 404 而不是 500
    let path = PathBuf::from(&book.file_path);
    if !path.exists() {
        tracing::error!(book_id, file_path = %book.file_path, "book file missing on disk");
        return Err(AppError::BookNotFound);
    }

    // ETag 基于内容 hash（上传时计算并持久化），稳定可缓存；
    // 命中 304 时无需打开文件
    let etag = format!("\"{}\"", book.content_hash);
    if let Some(if_none) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && etag_matches(if_none, &etag)
    {
        return Ok(StatusCode::NOT_MODIFIED.into_response());
    }

    let format = book.format;
    let disposition = build_content_disposition(
        params.download.unwrap_or(false),
        &book.title,
        book.author.as_deref(),
        book_id,
        format,
    );

    // 流式发送，避免大文件整本读入内存
    let file = fs::File::open(&path).await.map_err(AppError::Io)?;
    let content_length = file.metadata().await.map_err(AppError::Io)?.len();
    let stream = ReaderStream::new(file);

    Response::builder()
        .header(header::CONTENT_TYPE, format.mime_type())
        .header(header::CONTENT_LENGTH, content_length)
        .header(header::ETAG, etag.as_str())
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::CACHE_CONTROL, "private, no-cache")
        .body(Body::from_stream(stream))
        .map_err(|e| AppError::Internal(format!("failed to build download response: {e}")))
}

/// filename* 按 RFC 5987 编码时保留的安全字符（字母数字之外）
const FILENAME_STAR_SAFE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// 构造 Content-Disposition。
/// inline：浏览器内联展示（pdf 可直接预览）；
/// attachment：保存为文件，文件名为「书名 - 作者」（作者缺省/空白时仅书名）——
/// filename* 用 RFC 5987 百分号编码（支持任意 Unicode），filename 回退值仅保留
/// ASCII 安全字符，纯中文会退化为 book_{id}.{ext}。
fn build_content_disposition(
    attachment: bool,
    title: &str,
    author: Option<&str>,
    book_id: i64,
    format: BookFormat,
) -> String {
    let kind = if attachment {
        "attachment"
    } else {
        "inline"
    };
    let ext = format.ext();
    let stem = match author.map(str::trim).filter(|a| !a.is_empty()) {
        Some(a) => format!("{title} - {a}"),
        None => title.to_string(),
    };
    // quoted-string 内不允许 " 与 \，控制字符也不可行；空格等其余 ASCII 均合法。
    // 纯非 ASCII 的 stem（如中文书名）过滤后只剩 " - " 之类的分隔残片，无意义，
    // 要求至少含一个字母/数字，否则回退 book_{id}
    let fallback_stem: String = stem
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control() && *c != '"' && *c != '\\')
        .collect();
    let fallback = if fallback_stem.chars().any(|c| c.is_ascii_alphanumeric()) {
        format!("{fallback_stem}.{ext}")
    } else {
        format!("book_{book_id}.{ext}")
    };
    let full_name = format!("{stem}.{ext}");
    let encoded = percent_encoding::percent_encode(full_name.as_bytes(), FILENAME_STAR_SAFE);
    format!("{kind}; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

// 获取书籍内容（仅 txt 格式）
pub(super) async fn get_book_content(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Query(params): Query<ContentParams>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    // 查询书籍信息（回收站中的书不可读）
    let book = sqlx::query!(
        r#"SELECT user_id, file_path, is_public, total_chars, format AS "format!: BookFormat" FROM books WHERE id = ? AND deleted_at IS NULL"#,
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::BookNotFound)?;

    // 检查权限（所有者或公开书）
    ensure_book_access(auth.user_id, book.user_id, book.is_public)?;

    // 仅 txt 支持；epub/pdf 应该走 /file 下载原文件后客户端渲染
    if book.format != BookFormat::Txt {
        return Err(AppError::ContentEndpointNotSupported);
    }

    let content_length = book.total_chars.max(0);

    // 确保位置有效
    if params.position < 0 || params.position >= content_length {
        return Err(AppError::PositionExceedsRange);
    }
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let position = params.position as usize;

    // 计算要返回的内容长度
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let length = params.length.unwrap_or(4000).clamp(100, 10000) as usize;

    // 计算结束位置（基于已持久化的全文字符数，无需读全文）
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let end_pos = (position + length).min(content_length as usize);

    // 流式读取：避免大书全量进内存
    let content_slice = read_char_slice(&book.file_path, position, end_pos - position).await?;

    // 构建响应
    #[allow(clippy::cast_possible_wrap)]
    let resp_data = BookContentResponse {
        content: content_slice,
        next_position: end_pos as i64,
    };

    // 基于 book_id + position + length + 内容片段生成 ETag；同书同位置同长度则命中
    let etag_input = format!("{book_id}:{position}:{length}:{}", resp_data.content);
    let etag = make_etag(&etag_input);

    if let Some(if_none) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && etag_matches(if_none, &etag)
    {
        return Ok(StatusCode::NOT_MODIFIED.into_response());
    }

    Ok((
        [(header::ETAG, etag.as_str())],
        Json(ApiResponse::success(resp_data)),
    )
        .into_response())
}

/// 从文件中按字符偏移流式读取一段文本。
///
/// 实现要点：[`Utf8Chunks`] 保证每块在字符边界上结束，避免多字节字符被块边界切开。
async fn read_char_slice(
    path: &str,
    char_offset: usize,
    char_count: usize,
) -> Result<String, AppError> {
    let file = fs::File::open(path).await.map_err(AppError::Io)?;
    let mut chunks = Utf8Chunks::new(file);

    let mut collected = String::with_capacity(char_count * 4);
    let mut skipped = 0usize;
    let mut taken = 0usize;

    while taken < char_count {
        let chunk = chunks.next_chunk().await.map_err(AppError::Io)?;
        if chunk.is_empty() {
            break; // EOF
        }

        // 跳过尚未到达的字符偏移
        if skipped < char_offset {
            match char_boundary_at(chunk, char_offset - skipped) {
                // 目标落在本块内：定位到起始边界后开始收集
                Some(byte_off) => {
                    skipped = char_offset;
                    let take: String = chunk[byte_off..].chars().take(char_count - taken).collect();
                    taken += take.chars().count();
                    collected.push_str(&take);
                },
                // 目标在本块之后：整块跳过
                None => skipped += chunk.chars().count(),
            }
        } else {
            let take: String = chunk.chars().take(char_count - taken).collect();
            taken += take.chars().count();
            collected.push_str(&take);
        }
    }

    Ok(collected)
}

/// 第 `n` 个字符的字节偏移；字符数不足 `n + 1` 时返回 `None`
fn char_boundary_at(s: &str, n: usize) -> Option<usize> {
    s.char_indices().nth(n).map(|(i, _)| i)
}

// 跳转到指定章节
pub(super) async fn jump_to_chapter(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Query(params): Query<JumpToChapterParams>,
) -> Result<Json<ApiResponse<JumpToChapterResponse>>, AppError> {
    // 校验 chapter_id 有效
    if params.chapter_id <= 0 {
        return Err(AppError::ChapterNotFound);
    }

    // 检查书籍存在性与读权限（所有者或公开书）
    check_book_access(&pool, auth.user_id, book_id).await?;

    // 查询章节信息
    let chapter = sqlx::query!(
        "SELECT position FROM chapters WHERE id = ? AND book_id = ?",
        params.chapter_id,
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::ChapterNotFound)?;

    // 返回章节位置
    // 注意：position 字段对 txt 是字符偏移；对 epub 是 spine 索引
    Ok(Json(ApiResponse::success(JumpToChapterResponse {
        position: chapter.position,
    })))
}

#[cfg(test)]
mod tests {
    use super::{build_content_disposition, read_char_slice};
    use crate::{models::BookFormat, utils::utf8::UTF8_CHUNK_SIZE};

    #[test]
    fn content_disposition_ascii_fallback_keeps_printable() {
        let d = build_content_disposition(true, "My Book", Some("Jane Doe"), 7, BookFormat::Epub);
        assert_eq!(
            d,
            "attachment; filename=\"My Book - Jane Doe.epub\"; \
             filename*=UTF-8''My%20Book%20-%20Jane%20Doe.epub"
        );
        // 空白作者视作缺省：仅书名
        let d = build_content_disposition(true, "My Book", Some("  "), 7, BookFormat::Epub);
        assert_eq!(
            d,
            "attachment; filename=\"My Book.epub\"; filename*=UTF-8''My%20Book.epub"
        );
    }

    #[tokio::test]
    async fn read_char_slice_handles_multibyte_boundary() {
        // 构造跨块边界的多字节字符
        let mut content = String::new();
        for _ in 0..UTF8_CHUNK_SIZE - 1 {
            content.push('a');
        }
        content.push_str("世界🌍bb");
        let tmp = std::env::temp_dir().join("reader_test_read_char_slice.txt");
        tokio::fs::write(&tmp, &content).await.unwrap();

        let path = tmp.to_str().unwrap();

        // 1) 起点落在横跨块边界的字符上，取 4 个字符 → "世界🌍b"
        let s = read_char_slice(path, UTF8_CHUNK_SIZE - 1, 4).await.unwrap();
        assert_eq!(s, "世界🌍b", "char straddling chunk boundary");

        // 2) 从 emoji（CHUNK+1）取 3 个字符 → "🌍bb"
        let s = read_char_slice(path, UTF8_CHUNK_SIZE + 1, 3).await.unwrap();
        assert_eq!(s, "🌍bb");

        // 3) 跨边界的混合起点 → "a世界"
        let s = read_char_slice(path, UTF8_CHUNK_SIZE - 2, 3).await.unwrap();
        assert_eq!(s, "a世界");

        // 4) 全文长度
        let s = read_char_slice(path, 0, 100_000).await.unwrap();
        assert_eq!(s.chars().count(), content.chars().count());

        let _ = tokio::fs::remove_file(&tmp).await;
    }
}
