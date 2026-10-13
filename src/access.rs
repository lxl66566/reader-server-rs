//! 统一的访问权限与用户凭证校验
//!
//! 书籍读权限（所有者或公开书）与用户 token 状态（存在 / 版本号 / 封禁）
//! 的查询判断此前散落在各路由与 WS 握手中各写一份，此处收敛为唯一实现。

use sqlx::{Pool, Sqlite};

use crate::{error::AppError, models::UserStatus};

/// 检查用户对书籍的读访问权限（所有者或公开书）
///
/// 书不存在返回 404 `BookNotFound`，无权访问返回 403 `BookAccessDenied`。
pub async fn check_book_access(
    pool: &Pool<Sqlite>,
    user_id: i64,
    book_id: i64,
) -> Result<(), AppError> {
    let book = sqlx::query!("SELECT user_id, is_public FROM books WHERE id = ?", book_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::BookNotFound)?;

    ensure_book_access(user_id, book.user_id, book.is_public)
}

/// 已取到书籍行时的纯校验，语义与 [`check_book_access`] 一致（避免重复查询）
pub fn ensure_book_access(user_id: i64, owner_id: i64, is_public: bool) -> Result<(), AppError> {
    if owner_id != user_id && !is_public {
        return Err(AppError::BookAccessDenied);
    }
    Ok(())
}

/// 校验用户存在、token 版本号匹配、未被封禁
///
/// `AuthUser` 提取器、WS 握手与 WS 周期重验共用。
pub async fn check_user_token(
    pool: &Pool<Sqlite>,
    user_id: i64,
    token_version: i64,
) -> Result<(), AppError> {
    let user = sqlx::query!(
        r#"SELECT token_version, status AS "status: UserStatus" FROM users WHERE id = ?"#,
        user_id
    )
    .fetch_optional(pool)
    .await?;

    match user {
        // 用户已删除：token 自然失效，与封禁同为 401 语义（而非 404）
        None => Err(AppError::AccountDeleted),
        Some(row) if row.token_version != token_version => Err(AppError::TokenExpiredOrInvalid),
        Some(row) if row.status == UserStatus::Banned => Err(AppError::UserBanned),
        Some(_) => Ok(()),
    }
}
