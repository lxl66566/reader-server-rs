use axum::{
    Json,
    extract::{Path, State},
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
};

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
        "SELECT * FROM bookmarks WHERE user_id = ? AND book_id = ? ORDER BY position ASC",
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
    Path(book_id): Path<i64>,
    Json(req): Json<CreateBookmarkRequest>,
) -> Result<Json<ApiResponse<Bookmark>>, AppError> {
    if req.position < 0 {
        return Err(AppError::InvalidPosition);
    }
    check_book_access(&pool, &auth, book_id).await?;

    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let id = sqlx::query!(
        "INSERT INTO bookmarks (user_id, book_id, position, label, created_at) VALUES (?, ?, ?, ?, ?)",
        auth.user_id,
        book_id,
        req.position,
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

    Ok(Json(ApiResponse::success(bookmark)))
}

pub async fn delete_bookmark(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path((book_id, bookmark_id)): Path<(i64, i64)>,
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
        "SELECT * FROM annotations WHERE user_id = ? AND book_id = ? ORDER BY start_position ASC",
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
    Path(book_id): Path<i64>,
    Json(req): Json<CreateAnnotationRequest>,
) -> Result<Json<ApiResponse<Annotation>>, AppError> {
    if req.start_position < 0 {
        return Err(AppError::InvalidPosition);
    }
    // 高亮必须�?end_position，且 end > start
    if req.kind == AnnotationKind::Highlight {
        match req.end_position {
            Some(end) if end > req.start_position => {}
            _ => return Err(AppError::AnnotationRangeInvalid),
        }
    }

    check_book_access(&pool, &auth, book_id).await?;

    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let id = sqlx::query!(
        "INSERT INTO annotations (user_id, book_id, kind, start_position, end_position, excerpt, content, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        auth.user_id,
        book_id,
        req.kind,
        req.start_position,
        req.end_position,
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

    Ok(Json(ApiResponse::success(annotation)))
}

pub async fn update_annotation(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path((book_id, annotation_id)): Path<(i64, i64)>,
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

    Ok(Json(ApiResponse::<()>::message("Annotation updated")))
}

pub async fn delete_annotation(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path((book_id, annotation_id)): Path<(i64, i64)>,
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

    Ok(Json(ApiResponse::<()>::message("Annotation deleted")))
}
