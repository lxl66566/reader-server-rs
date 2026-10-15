use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::{SecondsFormat, Utc};
use sqlx::{Pool, Sqlite};

use super::helpers::{PaginationParams, delete_book_cascade, remove_book_file};
use crate::{
    auth::AuthUser,
    error::{ApiResponse, AppError},
    models::{BookFormat, TrashBookListItem, TrashBookListResponse},
};

// 删除书籍（软删除）：写 deleted_at 移入回收站，进度/批注等数据原样保留，
// 可经 /restore 恢复；彻底删除走 /purge 或管理员删除
pub(super) async fn delete_book(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // 时间戳与 created_at 同为 RFC3339；先取行做权限校验，非 owner 时不产生任何写入
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let mut tx = pool.begin().await?;
    let owner_id = sqlx::query_scalar!(
        "SELECT user_id FROM books WHERE id = ? AND deleted_at IS NULL",
        book_id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::BookNotFound)?;
    if owner_id != auth.user_id {
        return Err(AppError::BookDeleteDenied);
    }
    sqlx::query!("UPDATE books SET deleted_at = ? WHERE id = ?", now, book_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(Json(ApiResponse::<()>::message(
        "Moved to trash successfully",
    )))
}

// 回收站列表：仅当前用户已软删的书，按删除时间倒序，分页同书架
pub(super) async fn list_trash(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<ApiResponse<TrashBookListResponse>>, AppError> {
    let (limit, offset) = pagination.normalize(10, 50);

    let total = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM books WHERE user_id = ? AND deleted_at IS NOT NULL",
    )
    .bind(auth.user_id)
    .fetch_one(&pool)
    .await?;

    let books = sqlx::query!(
        r#"SELECT id, title, author, format AS "format!: BookFormat", deleted_at FROM books
         WHERE user_id = ? AND deleted_at IS NOT NULL
         ORDER BY deleted_at DESC, id DESC
         LIMIT ? OFFSET ?"#,
        auth.user_id,
        limit,
        offset
    )
    .fetch_all(&pool)
    .await?;

    let book_list: Vec<TrashBookListItem> = books
        .into_iter()
        // deleted_at 非空由 WHERE 保证
        .map(|b| TrashBookListItem {
            book_id: b.id,
            title: b.title,
            author: b.author,
            format: b.format,
            deleted_at: b.deleted_at.unwrap_or_default(),
        })
        .collect();

    Ok(Json(ApiResponse::success(TrashBookListResponse {
        total,
        books: book_list,
    })))
}

// 从回收站恢复：置回 deleted_at = NULL，进度/批注等数据未动过，自然恢复
pub(super) async fn restore_book(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let result = sqlx::query!(
        "UPDATE books SET deleted_at = NULL WHERE id = ? AND user_id = ? AND deleted_at IS NOT \
         NULL",
        book_id,
        auth.user_id
    )
    .execute(&pool)
    .await?;

    // 不存在 / 不属于本人 / 并发下已彻底删除 / 本就未删除，统一 404
    if result.rows_affected() == 0 {
        return Err(AppError::BookNotFound);
    }

    Ok(Json(ApiResponse::<()>::message("Restored successfully")))
}

// 彻底删除：级联清理 DB 行（章节/标签/书签/批注/进度/统计）+ 磁盘文件；
// 仅回收站中的书可彻底删除。离线缓存为前端 IndexedDB，由前端在成功后自行清理
pub(super) async fn purge_book(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    let mut tx = pool.begin().await?;

    // 仅回收站中的书可 purge；未软删的书不存在于回收站视图，统一 404
    // （SQLite EXISTS 返回 0/1 整数，按 i64 判非零）
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS(SELECT 1 FROM books WHERE id = ? AND user_id = ? AND deleted_at IS NOT NULL)"#,
        book_id,
        auth.user_id
    )
    .fetch_one(&mut *tx)
    .await?
    != 0;
    if !exists {
        return Err(AppError::BookNotFound);
    }

    let (_owner_id, file_path) = delete_book_cascade(&mut tx, book_id).await?;
    tx.commit().await?;

    remove_book_file(&file_path).await;

    Ok(Json(ApiResponse::<()>::message("Purged successfully")))
}
