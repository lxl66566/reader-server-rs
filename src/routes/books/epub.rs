// EPUB 懒加载资源端点：按 zip 条目路径返回解压后的单个条目。
// 前端 epub.js 以 OPF 目录模式按章取内容（打开书不再整本下载）；
// iframe 内浏览器直连的子资源（img/css/font）无法携带 Authorization 头，
// 故与 WS 握手一致改走 token，token 置于路径段以被 <base> 解析继承。

use std::{borrow::Cow, io::Read, path::PathBuf};

use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use sqlx::{Pool, Sqlite};

use crate::{
    auth::auth_user_by_token,
    config::Config,
    error::AppError,
    models::BookFormat,
    parser::epub::{MAX_EPUB_ENTRY_BYTES, expand_self_closed_raw_text, strip_external_font_face},
    routes::reading::etag_matches,
};

/// 条目名按扩展名映射 MIME；未识别类型按二进制流返回
fn entry_mime(name: &str) -> &'static str {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return "application/octet-stream";
    };
    match ext.to_ascii_lowercase().as_str() {
        "xhtml" => "application/xhtml+xml",
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "xml" => "text/xml; charset=utf-8",
        // OPF/NCX 供前端 requestMethod 以 text/xml 解析，Content-Type 仅作参考
        "opf" => "application/oebps-package+xml",
        "ncx" => "application/x-dtbncx+xml",
        "js" => "text/javascript",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 对文本条目做与上传解析同源的清洗：
/// - xhtml/html：自闭合 raw-text 标签展开（HTML 解析器不认，会吞掉整章正文）
/// - css：剥离引用非相对 url 的 @font-face（浏览器无法解析 file:/http: 源） 非文本条目、非 UTF-8
///   文本条目按原样返回（后者由前端解析时报错）
#[allow(clippy::case_sensitive_file_extension_comparisons)] // name 已 to_ascii_lowercase
fn sanitize_entry(name: &str, bytes: Vec<u8>) -> Vec<u8> {
    let lower = name.to_ascii_lowercase();
    let is_html = lower.ends_with(".xhtml") || lower.ends_with(".html") || lower.ends_with(".htm");
    let is_css = lower.ends_with(".css");
    if !is_html && !is_css {
        return bytes;
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => return e.into_bytes(),
    };
    let rewritten = if is_css {
        strip_external_font_face(&text)
    } else {
        expand_self_closed_raw_text(&text)
    };
    match rewritten {
        // 未改写时直接收回原 buffer，避免整段拷贝
        Cow::Borrowed(_) => text.into_bytes(),
        Cow::Owned(rewritten) => rewritten.into_bytes(),
    }
}

/// 从 epub zip 中解压单个条目，返回 (内容字节, CRC32 供 ETag 用)。
/// 条目解压大小复用上传时的 zip 炸弹上限
fn read_entry(path: &str, entry: &str) -> Result<(Vec<u8>, u32), AppError> {
    let file = std::fs::File::open(path).map_err(AppError::Io)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| AppError::FileParseError(format!("invalid EPUB zip: {e}")))?;
    let zip_entry = archive.by_name(entry).map_err(|_| AppError::BookNotFound)?;
    let crc = zip_entry.crc32();
    let mut buf = Vec::with_capacity(
        usize::try_from(zip_entry.size().min(MAX_EPUB_ENTRY_BYTES))
            .expect("entry size must fit in usize"),
    );
    // 多读 1 字节用于判断是否超限；take 保证不会把超限内容全量载入内存
    let mut limited = zip_entry.take(MAX_EPUB_ENTRY_BYTES + 1);
    limited
        .read_to_end(&mut buf)
        .map_err(|e| AppError::FileParseError(format!("corrupted EPUB entry: {e}")))?;
    if buf.len() as u64 > MAX_EPUB_ENTRY_BYTES {
        return Err(AppError::EpubContentTooLarge);
    }
    Ok((buf, crc))
}

/// 返回 EPUB zip 内的单个条目（解压后）
///
/// 路由：`GET /books/{book_id}/epub/{token}/{+entry}`
/// - token：用户 JWT（浏览器 iframe 子资源无法带 Authorization 头，走路径段）
/// - entry：zip 内条目路径（如 `OEBPS/content.opf`）
///
/// 响应：200 + 条目字节流（Content-Type 按扩展名、ETag 取条目 CRC32）；
/// 条目内容不可变，`Cache-Control: private, max-age=3600` 允许跨会话复用，
/// 过期后以 If-None-Match 命中 304。
pub(super) async fn serve_epub_entry(
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    Path((book_id, token, entry)): Path<(i64, String, String)>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let (auth, _) = auth_user_by_token(&pool, &config, &token).await?;

    let book = sqlx::query!(
        r#"SELECT user_id, file_path, is_public, format AS "format!: BookFormat" FROM books WHERE id = ? AND deleted_at IS NULL"#,
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::BookNotFound)?;

    // 权限：所有者或公开书
    crate::access::ensure_book_access(auth.user_id, book.user_id, book.is_public)?;

    // 仅 epub：其它格式没有 zip 条目语义
    if book.format != BookFormat::Epub {
        return Err(AppError::ContentEndpointNotSupported);
    }

    let path = PathBuf::from(&book.file_path);
    if !path.exists() {
        tracing::error!(book_id, file_path = %book.file_path, "book file missing on disk");
        return Err(AppError::BookNotFound);
    }

    // zip 为同步阻塞 IO + CPU 解压，置于 spawn_blocking
    let file_path = book.file_path.clone();
    let entry_name = entry.clone();
    let (bytes, crc) = tokio::task::spawn_blocking(move || read_entry(&file_path, &entry_name))
        .await
        .map_err(|e| AppError::Internal(format!("epub entry task failed: {e}")))??;

    let etag = format!("\"{crc:08x}\"");
    if let Some(if_none) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && etag_matches(if_none, &etag)
    {
        return Ok((
            [
                (header::ETAG, etag.as_str()),
                (header::CACHE_CONTROL, "private, max-age=3600"),
            ],
            StatusCode::NOT_MODIFIED,
        )
            .into_response());
    }

    let body = sanitize_entry(&entry, bytes);
    Response::builder()
        .header(header::CONTENT_TYPE, entry_mime(&entry))
        .header(header::CONTENT_LENGTH, body.len())
        .header(header::ETAG, etag.as_str())
        .header(header::CACHE_CONTROL, "private, max-age=3600")
        .body(Body::from(body))
        .map_err(|e| AppError::Internal(format!("failed to build epub entry response: {e}")))
}

#[cfg(test)]
mod tests {
    use super::sanitize_entry;

    #[test]
    fn sanitize_expands_self_closed_raw_text() {
        let out = sanitize_entry(
            "OEBPS/c1.xhtml",
            b"<html><body><script src=\"x.js\"/><p>a</p></body></html>".to_vec(),
        );
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("<script src=\"x.js\"></script>"));
    }

    #[test]
    fn sanitize_strips_external_font_face_keeps_relative() {
        let css = b"\
            @font-face { font-family: a; src: url(file:///x.woff); }\n\
            @font-face { font-family: b; src: url(fonts/b.woff); }\n";
        let out = sanitize_entry("OEBPS/style.css", css.to_vec());
        let s = String::from_utf8(out).unwrap();
        assert!(!s.contains("file:"));
        assert!(s.contains("url(fonts/b.woff)"));
    }

    #[test]
    fn sanitize_keeps_binary_entries_untouched() {
        // 无需清洗的类型：返回原字节
        let data = b"\xff\xd8\xff".to_vec();
        let out = sanitize_entry("OEBPS/cover.jpg", data.clone());
        assert_eq!(out, data);
    }

    #[test]
    fn sanitize_invalid_utf8_text_entry_kept_as_is() {
        let out = sanitize_entry("OEBPS/x.xhtml", vec![0xff, 0xfe]);
        assert_eq!(out, vec![0xff, 0xfe]);
    }
}
