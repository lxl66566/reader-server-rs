use axum::{
    Json,
    extract::{Path, State},
};
use sqlx::{Pool, Sqlite};

use super::helpers::{TAGS_MAX_COUNT, clean_tags};
use crate::{
    access::check_book_access,
    auth::AuthUser,
    error::{ApiResponse, AppError},
    models::{BookTagsResponse, SetBookTagsRequest},
};

// ============ 书籍标签 ============

/// 验证当前用户对书籍有写权限（必须是 owner；公开书不可改标签）
async fn require_book_owner(
    pool: &Pool<Sqlite>,
    auth: &AuthUser,
    book_id: i64,
) -> Result<(), AppError> {
    // 回收站中的书不可改标签
    let owner_id: Option<i64> =
        sqlx::query_scalar("SELECT user_id FROM books WHERE id = ? AND deleted_at IS NULL")
            .bind(book_id)
            .fetch_optional(pool)
            .await?;
    match owner_id {
        None => Err(AppError::BookNotFound),
        Some(uid) if uid == auth.user_id => Ok(()),
        Some(_) => Err(AppError::BookModifyDenied),
    }
}

// 获取书籍标签
pub(super) async fn get_book_tags(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
) -> Result<Json<ApiResponse<BookTagsResponse>>, AppError> {
    // 读权限：所有者或公开书
    check_book_access(&pool, auth.user_id, book_id).await?;

    let tags: Vec<String> = sqlx::query_scalar(
        "SELECT t.name FROM tags t JOIN book_tags bt ON bt.tag_id = t.id WHERE bt.book_id = ? \
         ORDER BY t.name",
    )
    .bind(book_id)
    .fetch_all(&pool)
    .await?;

    Ok(Json(ApiResponse::success(BookTagsResponse { tags })))
}

// 设置书籍标签（覆盖式）：事务内删旧建新；标签字典不存在则自动创建
pub(super) async fn set_book_tags(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Json(req): Json<SetBookTagsRequest>,
) -> Result<Json<ApiResponse<BookTagsResponse>>, AppError> {
    require_book_owner(&pool, &auth, book_id).await?;

    // 数量上限：超限直接拒绝（400），不做静默截断
    if req.tags.len() > TAGS_MAX_COUNT {
        return Err(AppError::SettingOutOfRange("tags"));
    }

    let tags = clean_tags(req.tags);

    let mut tx = pool.begin().await?;

    // 清掉旧的关联
    sqlx::query!("DELETE FROM book_tags WHERE book_id = ?", book_id)
        .execute(&mut *tx)
        .await?;

    // 为每个标签在字典里 upsert（INSERT OR IGNORE）并取出 id
    for tag in &tags {
        sqlx::query!("INSERT OR IGNORE INTO tags (name) VALUES (?)", tag)
            .execute(&mut *tx)
            .await?;
        let tag_id: i64 = sqlx::query_scalar!("SELECT id FROM tags WHERE name = ?", tag)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query!(
            "INSERT OR IGNORE INTO book_tags (book_id, tag_id) VALUES (?, ?)",
            book_id,
            tag_id
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(Json(ApiResponse::success(BookTagsResponse { tags })))
}
