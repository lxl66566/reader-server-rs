//! 临时 zip 的构建与流式响应
//!
//! 导出 / 全局备份可能打包大量书籍文件，内存里拼 zip 会被反复拉取打成 OOM。
//! 统一方案：先在 spawn_blocking 中把 zip 流式写入 /tmp 临时文件
//! （书籍文件用 io::copy 搬运，内存占用与书数量无关），再以
//! Body::from_stream 回传；响应结束或客户端断开时删除临时文件。

use std::{
    path::PathBuf,
    pin::Pin,
    task::{Context, Poll},
};

use axum::{body::Body, http::header, response::Response};
use bytes::Bytes;
use futures_util::Stream;
use tokio_util::io::ReaderStream;

use super::generate_uuid;
use crate::error::AppError;

/// 统一 zip 写入错误映射（服务端组装失败属内部错误）。
/// 按值接收以匹配 map_err 的函数形态
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn zip_write_err(e: zip::result::ZipError) -> AppError {
    AppError::Internal(format!("zip error: {e}"))
}

/// 在 spawn_blocking 中把 zip 写入临时文件，返回 (路径, 文件大小)。
/// 构建失败时清理临时文件
pub async fn write_temp_zip(
    build: impl FnOnce(&mut zip::ZipWriter<std::fs::File>) -> Result<(), AppError> + Send + 'static,
) -> Result<(PathBuf, u64), AppError> {
    let path = std::env::temp_dir().join(format!("reader-zip-{}.zip", generate_uuid()));
    let build_path = path.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<u64, AppError> {
        let file = std::fs::File::create(&build_path).map_err(AppError::Io)?;
        let mut zip = zip::ZipWriter::new(file);
        build(&mut zip)?;
        // finish 落盘 central directory 并 flush
        zip.finish().map_err(zip_write_err)?;
        Ok(std::fs::metadata(&build_path).map_err(AppError::Io)?.len())
    })
    .await
    .map_err(|e| AppError::Internal(format!("zip task failed: {e}")))?;

    match result {
        Ok(len) => Ok((path, len)),
        Err(e) => {
            let _ = tokio::fs::remove_file(&path).await;
            Err(e)
        },
    }
}

/// 流式发送临时 zip：Content-Length 取文件大小，响应体为文件流。
/// 流结束或响应被丢弃（客户端断开）时删除临时文件
pub async fn stream_temp_zip(
    path: PathBuf,
    content_disposition: String,
) -> Result<Response, AppError> {
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => {
            // 打开失败：所有权未移交清理流，此处删除避免泄漏
            let _ = tokio::fs::remove_file(&path).await;
            return Err(AppError::Io(e));
        },
    };
    let len = file.metadata().await.map_err(AppError::Io)?.len();
    let stream = TempZipStream {
        inner: ReaderStream::new(file),
        path,
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .header(header::CONTENT_DISPOSITION, content_disposition)
        .header(header::CONTENT_LENGTH, len)
        .body(Body::from_stream(stream))
        .map_err(|e| AppError::Internal(format!("failed to build zip response: {e}")))
}

/// ReaderStream 包装：Drop（流耗尽 / 响应被丢弃）时清理临时文件。
/// ReaderStream 在底层 File 为 Unpin 时亦为 Unpin，可直接 Pin::new 投影
struct TempZipStream {
    inner: ReaderStream<tokio::fs::File>,
    path: PathBuf,
}

impl Stream for TempZipStream {
    type Item = std::io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

impl Drop for TempZipStream {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(error = %e, path = %self.path.display(), "failed to remove temp zip");
        }
    }
}
