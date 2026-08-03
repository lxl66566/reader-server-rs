use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use chrono::{SecondsFormat, Utc};
use sqlx::{Pool, Sqlite};

use crate::{
    auth::AuthUser,
    error::{ApiResponse, AppError},
    models::{
        Annotation, AnnotationKind, Bookmark, CreateAnnotationRequest, CreateBookmarkRequest,
        ListAnnotationsResponse, ListBookmarksResponse, UpdateAnnotationRequest,
    },
    ws::{manager::ConnectionRegistry, protocol::ServerMessage},
};

/// 从请求头中提取可选的 `device_id`（用于 ws 广播时排除发起方设备）。
/// 客户端在 POST/PUT/DELETE 时可附带 `X-Device-Id` 头来精确避免回环通知。
fn device_id_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers.get("x-device-id").and_then(|v| v.to_str().ok())
}

/// 校验进度值在 [0, 1]，否则返回错误
fn validate_percent(p: f64) -> Result<(), AppError> {
    if !(0.0..=1.0).contains(&p) {
        return Err(AppError::InvalidPercent);
    }
    Ok(())
}

/// 广播书签变更给订阅了同 (user, book) 的其它设备。
/// `exclude_device` 通常是发起本次 HTTP 请求的 `device_id`（可选）。
async fn broadcast_bookmark_changed(
    ws: &Arc<ConnectionRegistry>,
    user_id: i64,
    book_id: i64,
    exclude_device: Option<&str>,
    action: &str,
    bookmark_id: i64,
) {
    ws.broadcast_progress(
        user_id,
        book_id,
        exclude_device,
        ServerMessage::BookmarkChanged {
            book_id,
            action: action.to_string(),
            bookmark_id,
        },
    )
    .await;
}

/// 广播批注变更给订阅了同 (user, book) 的其它设备。
async fn broadcast_annotation_changed(
    ws: &Arc<ConnectionRegistry>,
    user_id: i64,
    book_id: i64,
    exclude_device: Option<&str>,
    action: &str,
    annotation_id: i64,
) {
    ws.broadcast_progress(
        user_id,
        book_id,
        exclude_device,
        ServerMessage::AnnotationChanged {
            book_id,
            action: action.to_string(),
            annotation_id,
        },
    )
    .await;
}

/// 检查用户对书籍的访问权限（所有者或公开书），失败返回对应 `AppError`
async fn check_book_access(
    pool: &Pool<Sqlite>,
    auth: &AuthUser,
    book_id: i64,
) -> Result<(), AppError> {
    let book = sqlx::query!("SELECT user_id, is_public FROM books WHERE id = ?", book_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::BookNotFound)?;

    if book.user_id != auth.user_id && !book.is_public {
        return Err(AppError::BookAccessDenied);
    }
    Ok(())
}

// ============ 书签 ============

pub async fn list_bookmarks(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
) -> Result<Json<ApiResponse<ListBookmarksResponse>>, AppError> {
    check_book_access(&pool, &auth, book_id).await?;

    let bookmarks = sqlx::query_as::<_, Bookmark>(
        "SELECT * FROM bookmarks WHERE user_id = ? AND book_id = ? ORDER BY percent ASC, id ASC",
    )
    .bind(auth.user_id)
    .bind(book_id)
    .fetch_all(&pool)
    .await?;

    Ok(Json(ApiResponse::success(ListBookmarksResponse {
        bookmarks,
    })))
}

pub async fn create_bookmark(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    State(ws): State<Arc<ConnectionRegistry>>,
    Path(book_id): Path<i64>,
    headers: HeaderMap,
    Json(req): Json<CreateBookmarkRequest>,
) -> Result<Json<ApiResponse<Bookmark>>, AppError> {
    if req.locator.trim().is_empty() {
        return Err(AppError::EmptyLocator);
    }
    validate_percent(req.percent)?;
    check_book_access(&pool, &auth, book_id).await?;

    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let id = sqlx::query!(
        "INSERT INTO bookmarks (user_id, book_id, locator, percent, label, created_at) VALUES (?, ?, ?, ?, ?, ?)",
        auth.user_id,
        book_id,
        req.locator,
        req.percent,
        req.label,
        now,
    )
    .execute(&pool)
    .await?
    .last_insert_rowid();

    let bookmark = sqlx::query_as::<_, Bookmark>("SELECT * FROM bookmarks WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await?;

    broadcast_bookmark_changed(
        &ws,
        auth.user_id,
        book_id,
        device_id_from_headers(&headers),
        "created",
        id,
    )
    .await;

    Ok(Json(ApiResponse::success(bookmark)))
}

pub async fn delete_bookmark(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    State(ws): State<Arc<ConnectionRegistry>>,
    Path((book_id, bookmark_id)): Path<(i64, i64)>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // 通过 user_id 限制确保只能删自己的书签
    let result = sqlx::query!(
        "DELETE FROM bookmarks WHERE id = ? AND user_id = ? AND book_id = ?",
        bookmark_id,
        auth.user_id,
        book_id,
    )
    .execute(&pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::BookmarkNotFound);
    }

    broadcast_bookmark_changed(
        &ws,
        auth.user_id,
        book_id,
        device_id_from_headers(&headers),
        "deleted",
        bookmark_id,
    )
    .await;

    Ok(Json(ApiResponse::<()>::message("Bookmark deleted")))
}

// ============ 批注 ============

pub async fn list_annotations(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
) -> Result<Json<ApiResponse<ListAnnotationsResponse>>, AppError> {
    check_book_access(&pool, &auth, book_id).await?;

    let annotations = sqlx::query_as::<_, Annotation>(
        "SELECT * FROM annotations WHERE user_id = ? AND book_id = ? ORDER BY start_percent ASC, id ASC",
    )
    .bind(auth.user_id)
    .bind(book_id)
    .fetch_all(&pool)
    .await?;

    Ok(Json(ApiResponse::success(ListAnnotationsResponse {
        annotations,
    })))
}

pub async fn create_annotation(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    State(ws): State<Arc<ConnectionRegistry>>,
    Path(book_id): Path<i64>,
    headers: HeaderMap,
    Json(req): Json<CreateAnnotationRequest>,
) -> Result<Json<ApiResponse<Annotation>>, AppError> {
    if req.start_locator.trim().is_empty() {
        return Err(AppError::EmptyLocator);
    }
    validate_percent(req.start_percent)?;

    // 高亮必须有 end_locator 与 end_percent，且 end > start
    if req.kind == AnnotationKind::Highlight {
        let end_ok = match (&req.end_locator, req.end_percent) {
            (Some(end), Some(ep)) if !end.trim().is_empty() && ep > req.start_percent => {
                validate_percent(ep)?;
                true
            }
            _ => false,
        };
        if !end_ok {
            return Err(AppError::AnnotationRangeInvalid);
        }
    } else if let Some(ep) = req.end_percent {
        validate_percent(ep)?;
    }

    check_book_access(&pool, &auth, book_id).await?;

    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let id = sqlx::query!(
        "INSERT INTO annotations (user_id, book_id, kind, start_locator, end_locator, start_percent, end_percent, excerpt, content, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        auth.user_id,
        book_id,
        req.kind,
        req.start_locator,
        req.end_locator,
        req.start_percent,
        req.end_percent,
        req.excerpt,
        req.content,
        now,
        now,
    )
    .execute(&pool)
    .await?
    .last_insert_rowid();

    let annotation = sqlx::query_as::<_, Annotation>("SELECT * FROM annotations WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await?;

    broadcast_annotation_changed(
        &ws,
        auth.user_id,
        book_id,
        device_id_from_headers(&headers),
        "created",
        id,
    )
    .await;

    Ok(Json(ApiResponse::success(annotation)))
}

pub async fn update_annotation(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    State(ws): State<Arc<ConnectionRegistry>>,
    Path((book_id, annotation_id)): Path<(i64, i64)>,
    headers: HeaderMap,
    Json(req): Json<UpdateAnnotationRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // 至少一个字段要更新
    if req.excerpt.is_none() && req.content.is_none() {
        return Ok(Json(ApiResponse::<()>::message("No updates")));
    }

    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);

    let mut qb = sqlx::QueryBuilder::new("UPDATE annotations SET updated_at = ");
    qb.push_bind(&now);
    if let Some(excerpt) = &req.excerpt {
        qb.push(", excerpt = ").push_bind(excerpt);
    }
    if let Some(content) = &req.content {
        qb.push(", content = ").push_bind(content);
    }
    qb.push(" WHERE id = ")
        .push_bind(annotation_id)
        .push(" AND user_id = ")
        .push_bind(auth.user_id)
        .push(" AND book_id = ")
        .push_bind(book_id);

    let result = qb.build().execute(&pool).await?;
    if result.rows_affected() == 0 {
        return Err(AppError::AnnotationNotFound);
    }

    broadcast_annotation_changed(
        &ws,
        auth.user_id,
        book_id,
        device_id_from_headers(&headers),
        "updated",
        annotation_id,
    )
    .await;

    Ok(Json(ApiResponse::<()>::message("Annotation updated")))
}

pub async fn delete_annotation(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    State(ws): State<Arc<ConnectionRegistry>>,
    Path((book_id, annotation_id)): Path<(i64, i64)>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let result = sqlx::query!(
        "DELETE FROM annotations WHERE id = ? AND user_id = ? AND book_id = ?",
        annotation_id,
        auth.user_id,
        book_id,
    )
    .execute(&pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::AnnotationNotFound);
    }

    broadcast_annotation_changed(
        &ws,
        auth.user_id,
        book_id,
        device_id_from_headers(&headers),
        "deleted",
        annotation_id,
    )
    .await;

    Ok(Json(ApiResponse::<()>::message("Annotation deleted")))
}
