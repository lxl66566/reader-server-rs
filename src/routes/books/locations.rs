// EPUB locations 预生成端点：服务端按 epub.js 兼容规则生成全书 locations
// 数组，前端经 `book.locations.load()` 注入，省去打开书后为计算百分比逐章
// 拉取（一章一请求 + 100ms 停顿）。
// 结果缓存在书籍文件旁的 sidecar 文件：书籍文件以 uuid 命名且不可变，
// 缓存无失效问题；写失败仅告警（下次惰性重建）。

use std::path::Path as FilePath;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use sqlx::{Pool, Sqlite};

use crate::{
    access::ensure_book_access,
    auth::AuthUser,
    error::{ApiResponse, AppError},
    models::BookFormat,
    parser::epub_locations::generate_locations,
    routes::reading::etag_matches,
    utils::make_etag,
};

const CACHE_CONTROL: &str = "private, max-age=3600";

/// 读缓存或惰性生成 locations 响应体（阻塞实现，置于 spawn_blocking）。
/// 缓存半写（宕机截断）时 JSON 结构校验不过，走重新生成
fn locations_body(file_path: &str) -> Result<Vec<u8>, AppError> {
    let sidecar = format!("{file_path}.locations.json");
    if let Ok(cached) = std::fs::read(&sidecar)
        && serde_json::from_slice::<serde_json::Value>(&cached).is_ok()
    {
        return Ok(cached);
    }
    let data = std::fs::read(file_path).map_err(AppError::Io)?;
    let locations = generate_locations(&data)?;
    let body = serde_json::to_vec(&ApiResponse::success(locations))
        .map_err(|e| AppError::Internal(format!("failed to serialize locations: {e}")))?;
    if let Err(e) = std::fs::write(&sidecar, &body) {
        tracing::warn!(sidecar = %sidecar, error = %e, "failed to write locations cache");
    }
    Ok(body)
}

/// 获取 EPUB 全书 locations（epub.js 兼容 CFI 数组）
///
/// 路由：`GET /books/{book_id}/locations`
///
/// 响应：200 + `{"code":0,"data":[...]}`；ETag 基于响应体内容，
/// `Cache-Control: private, max-age=3600`，过期后 If-None-Match 命中 304。
/// 生成失败（书损坏）返回 500，前端回退原生逐章生成。
pub(super) async fn get_book_locations(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let book = sqlx::query!(
        r#"SELECT user_id, file_path, is_public, format AS "format!: BookFormat" FROM books WHERE id = ? AND deleted_at IS NULL"#,
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::BookNotFound)?;

    // 权限：所有者或公开书
    ensure_book_access(auth.user_id, book.user_id, book.is_public)?;

    // locations 按 zip 条目语义生成，仅 epub 有意义
    if book.format != BookFormat::Epub {
        return Err(AppError::ContentEndpointNotSupported);
    }

    if !FilePath::new(&book.file_path).exists() {
        tracing::error!(book_id, file_path = %book.file_path, "book file missing on disk");
        return Err(AppError::BookNotFound);
    }

    // 全书读取 + 解析为 CPU 密集任务
    let file_path = book.file_path.clone();
    let body = tokio::task::spawn_blocking(move || locations_body(&file_path))
        .await
        .map_err(|e| AppError::Internal(format!("locations task failed: {e}")))??;

    let etag = make_etag(&String::from_utf8_lossy(&body));
    if let Some(if_none) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && etag_matches(if_none, &etag)
    {
        return Ok((
            [
                (header::ETAG, etag.as_str()),
                (header::CACHE_CONTROL, CACHE_CONTROL),
            ],
            StatusCode::NOT_MODIFIED,
        )
            .into_response());
    }

    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_LENGTH, body.len())
        .header(header::ETAG, etag.as_str())
        .header(header::CACHE_CONTROL, CACHE_CONTROL)
        .body(Body::from(body))
        .map_err(|e| AppError::Internal(format!("failed to build locations response: {e}")))
}

#[cfg(test)]
mod tests {
    use super::locations_body;
    use crate::error::AppError;

    /// 构造最小 EPUB（内容无需合法章节，locations 生成走兜底也行）
    fn build_epub() -> Vec<u8> {
        use std::io::Write as _;

        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            let deflated = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file("mimetype", stored).unwrap();
            writer.write_all(b"application/epub+zip").unwrap();
            writer
                .start_file("META-INF/container.xml", deflated)
                .unwrap();
            writer
                .write_all(
                    br#"<container><rootfiles><rootfile full-path="b.opf"/></rootfiles></container>"#,
                )
                .unwrap();
            writer.start_file("b.opf", deflated).unwrap();
            writer
                .write_all(
                    br#"<package><manifest><item id="c" href="c.xhtml"/></manifest><spine><itemref idref="c"/></spine></package>"#,
                )
                .unwrap();
            writer.start_file("c.xhtml", deflated).unwrap();
            writer
                .write_all(b"<html><body><p>hello locations</p></body></html>")
                .unwrap();
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn locations_body_writes_and_reads_cache() {
        let dir = std::env::temp_dir().join("reader_test_locations");
        std::fs::create_dir_all(&dir).unwrap();
        let epub = dir.join("book.epub");
        std::fs::write(&epub, build_epub()).unwrap();
        let sidecar = dir.join("book.epub.locations.json");

        let first = locations_body(epub.to_str().unwrap()).unwrap();
        assert!(first.starts_with(br#"{"code":0,"message":"success","data":"#));
        assert!(sidecar.exists(), "cache file should be written");

        // 二次调用走缓存：内容一致
        let second = locations_body(epub.to_str().unwrap()).unwrap();
        assert_eq!(first, second);

        // 缓存损坏（截断）时重新生成
        std::fs::write(&sidecar, &first[..first.len() / 2]).unwrap();
        let third = locations_body(epub.to_str().unwrap()).unwrap();
        assert_eq!(first, third);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locations_body_missing_file_is_io_error() {
        let err = locations_body("/tmp/definitely-missing-uuid.epub");
        assert!(matches!(err, Err(AppError::Io(_))));
    }
}
