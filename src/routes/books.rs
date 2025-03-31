use std::path::PathBuf;

use axum::{
    extract::{multipart::Multipart, Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use rand::prelude::IndexedRandom;
use serde::Deserialize;
use sqlx::{Pool, Sqlite};
use tokio::{fs, io::AsyncWriteExt};

use crate::{
    auth::AuthUser,
    config::Config,
    error::{ApiResponse, AppError},
    models::{
        Book, BookContentResponse, BookDetailResponse, BookListItem, ChapterResponse,
        PublicBookListItem, UpdateBookRequest, UploadBookResponse,
    },
    utils::{extract_chapters, generate_uuid},
};

// 分页查询参数
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

// 获取内容查询参数
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

// 随机公开书籍查询参数
#[derive(Debug, Deserialize)]
pub struct RandomPublicParams {
    pub count: Option<i64>,
}

// 书籍路由
pub fn routes() -> Router<crate::app::AppState> {
    Router::new()
        .route("/upload", post(upload_book))
        .route("/", get(list_books))
        .route(
            "/{book_id}",
            get(get_book_detail).put(update_book).delete(delete_book),
        )
        .route("/{book_id}/content", get(get_book_content))
        .route("/{book_id}/jump_to_chapter", get(jump_to_chapter))
        .route("/public", get(list_public_books))
        .route("/random_public", get(get_random_public_books))
}

// 上传书籍
async fn upload_book(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    mut multipart: Multipart,
) -> Result<Json<ApiResponse<UploadBookResponse>>, AppError> {
    // 解析multipart表单数据
    let mut title = None;
    let mut author = None;
    let mut is_public = false;
    let mut file_content = None;
    let mut file_name = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("解析表单数据失败: {}", e)))?
    {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "title" => {
                title = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| AppError::BadRequest(format!("读取标题失败: {}", e)))?,
                );
            }
            "author" => {
                author = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| AppError::BadRequest(format!("读取作者失败: {}", e)))?,
                );
            }
            "is_public" => {
                let value = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("读取公开状态失败: {}", e)))?;
                is_public = value == "true" || value == "1";
            }
            "file" => {
                file_name = field.file_name().map(|s| s.to_string());
                file_content = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| AppError::BadRequest(format!("读取文件内容失败: {}", e)))?,
                );
            }
            _ => {}
        }
    }

    // 验证必要字段
    let title = title.ok_or_else(|| AppError::Validation("标题不能为空".to_string()))?;
    let file_content =
        file_content.ok_or_else(|| AppError::Validation("文件不能为空".to_string()))?;

    // 验证文件大小
    if file_content.len() > 10 * 1024 * 1024 {
        return Err(AppError::Validation("文件大小不能超过10MB".to_string()));
    }

    // 验证文件格式
    if !file_name
        .unwrap_or_default()
        .to_lowercase()
        .ends_with(".txt")
    {
        return Err(AppError::Validation("只支持TXT格式的书籍".to_string()));
    }

    // 将文件内容转换为UTF-8文本
    let content = String::from_utf8(file_content.to_vec())
        .map_err(|_| AppError::Validation("文件编码不是有效的UTF-8".to_string()))?;

    // 提取章节
    let chapters = extract_chapters(&content);

    // 生成唯一文件名
    let file_id = generate_uuid();
    let file_path = PathBuf::from(&config.storage.book_dir).join(format!("{}.txt", file_id));
    let file_path_string = file_path.to_string_lossy();

    // 保存文件
    let mut file = fs::File::create(&file_path).await.map_err(AppError::Io)?;
    file.write_all(content.as_bytes())
        .await
        .map_err(AppError::Io)?;

    // 将书籍信息保存到数据库
    let book_id = sqlx::query!(
        "INSERT INTO books (user_id, title, author, file_path, is_public) VALUES (?, ?, ?, ?, ?)",
        auth.user_id,
        title,
        author,
        file_path_string,
        is_public
    )
    .execute(&pool)
    .await?
    .last_insert_rowid();

    // 保存章节信息
    let mut chapter_responses = Vec::new();
    for (chapter_title, position) in chapters {
        let position_temp = position as i64;
        let chapter_id = sqlx::query!(
            "INSERT INTO chapters (book_id, title, position) VALUES (?, ?, ?)",
            book_id,
            chapter_title,
            position_temp
        )
        .execute(&pool)
        .await?
        .last_insert_rowid();

        chapter_responses.push(ChapterResponse {
            chapter_id,
            title: chapter_title,
            position: position as i64,
        });
    }

    // 创建初始阅读进度
    sqlx::query!(
        "INSERT INTO reading_progress (user_id, book_id) VALUES (?, ?)",
        auth.user_id,
        book_id
    )
    .execute(&pool)
    .await?;

    // 返回响应
    let response = UploadBookResponse {
        book_id,
        title,
        author,
        chapters: chapter_responses,
    };

    Ok(Json(ApiResponse::success(response)))
}

// 获取用户书籍列表
async fn list_books(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    // 解析分页参数
    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(10);
    let offset = (page - 1) * limit;

    // 获取总数
    let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM books WHERE user_id = ?")
        .bind(auth.user_id)
        .fetch_one(&pool)
        .await?;

    // 获取书籍列表
    let books = sqlx::query!(
        r#"
        SELECT b.id, b.title, b.author, b.is_public, b.created_at,
               rp.position, rp.reading_time, rp.last_read_at
        FROM books b
        LEFT JOIN reading_progress rp ON b.id = rp.book_id AND rp.user_id = ?
        WHERE b.user_id = ?
        ORDER BY rp.last_read_at DESC NULLS LAST, b.created_at DESC
        LIMIT ? OFFSET ?
        "#,
        auth.user_id,
        auth.user_id,
        limit,
        offset
    )
    .fetch_all(&pool)
    .await?;

    // 构建响应
    let book_list: Vec<BookListItem> = books
        .into_iter()
        .map(|book| BookListItem {
            book_id: book.id,
            title: book.title,
            author: book.author,
            is_public: book.is_public,
            created_at: book.created_at,
            last_read_at: book.last_read_at,
            position: book.position.unwrap_or(0),
            reading_time: book.reading_time.unwrap_or(0),
        })
        .collect();

    Ok(Json(ApiResponse::success(serde_json::json!({
        "total": total,
        "books": book_list
    }))))
}

// 获取书籍详情
async fn get_book_detail(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
) -> Result<Json<ApiResponse<BookDetailResponse>>, AppError> {
    // 查询书籍信息
    let book = sqlx::query_as::<_, Book>("SELECT * FROM books WHERE id = ?")
        .bind(book_id)
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| AppError::NotFound("书籍不存在".to_string()))?;

    // 检查权限
    if book.user_id != auth.user_id {
        // 如果不是书籍所有者，检查书籍是否公开
        if !book.is_public {
            return Err(AppError::Forbidden("无权访问该书籍".to_string()));
        }
    }

    // 查询章节信息
    let chapters = sqlx::query!(
        "SELECT id, title, position FROM chapters WHERE book_id = ? ORDER BY position",
        book_id
    )
    .fetch_all(&pool)
    .await?;

    // 查询阅读进度
    let progress = sqlx::query!(
        r#"SELECT position, reading_time, last_read_at FROM reading_progress 
         WHERE user_id = ? AND book_id = ?"#,
        auth.user_id,
        book_id
    )
    .fetch_optional(&pool)
    .await?;

    // 如果没有阅读进度，创建一个
    let (position, reading_time, last_read_at) = if let Some(p) = progress {
        (p.position, p.reading_time, p.last_read_at)
    } else {
        // 如果是公开书籍，为当前用户创建进度记录
        if book.user_id != auth.user_id {
            sqlx::query!(
                "INSERT INTO reading_progress (user_id, book_id) VALUES (?, ?)",
                auth.user_id,
                book_id
            )
            .execute(&pool)
            .await?;
        }
        (0, 0, None)
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
        created_at: book.created_at,
        last_read_at,
        position,
        reading_time,
        chapters: chapter_responses,
    };

    Ok(Json(ApiResponse::success(response)))
}

// 更新书籍信息
async fn update_book(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Json(req): Json<UpdateBookRequest>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // 检查书籍是否存在并属于当前用户
    let book = sqlx::query!("SELECT user_id FROM books WHERE id = ?", book_id)
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| AppError::NotFound("书籍不存在".to_string()))?;

    // 验证权限
    if book.user_id != auth.user_id {
        return Err(AppError::Forbidden("无权修改该书籍".to_string()));
    }

    // 构建更新SQL
    let mut updates = Vec::new();
    let mut params = Vec::new();

    if let Some(title) = &req.title {
        updates.push("title = ?");
        params.push(title.as_str());
    }

    if let Some(author) = &req.author {
        updates.push("author = ?");
        params.push(author.as_str());
    }

    if let Some(is_public) = &req.is_public {
        updates.push("is_public = ?");
        params.push(if *is_public { "true" } else { "false" });
    }

    // 如果没有需要更新的字段，直接返回成功
    if updates.is_empty() {
        return Ok(Json(ApiResponse::<()>::message("无更新内容")));
    }

    // 构建SQL语句
    let sql = format!("UPDATE books SET {} WHERE id = ?", updates.join(", "));

    // 执行更新
    let mut query = sqlx::query(&sql);
    for param in params {
        query = query.bind(param);
    }
    query = query.bind(book_id);

    query.execute(&pool).await?;

    Ok(Json(ApiResponse::<()>::message("更新成功")))
}

// 删除书籍
async fn delete_book(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    State(_config): State<Config>,
    Path(book_id): Path<i64>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // 开始事务
    let mut tx = pool.begin().await?;

    // 检查书籍是否存在并属于当前用户
    let book = sqlx::query!("SELECT user_id, file_path FROM books WHERE id = ?", book_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("书籍不存在".to_string()))?;

    // 验证权限
    if book.user_id != auth.user_id {
        return Err(AppError::Forbidden("无权删除该书籍".to_string()));
    }

    // 删除书籍文件
    let file_path = book.file_path;
    if PathBuf::from(&file_path).exists() {
        fs::remove_file(&file_path).await.map_err(AppError::Io)?;
    }

    // 删除数据库中的书籍记录
    // 注意：由于设置了外键约束，章节和阅读进度会自动删除
    sqlx::query!("DELETE FROM books WHERE id = ?", book_id)
        .execute(&mut *tx)
        .await?;

    // 提交事务
    tx.commit().await?;

    Ok(Json(ApiResponse::<()>::message("删除成功")))
}

// 获取书籍内容
async fn get_book_content(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Query(params): Query<ContentParams>,
) -> Result<Json<ApiResponse<BookContentResponse>>, AppError> {
    // 查询书籍信息
    let book = sqlx::query!(
        "SELECT user_id, file_path, is_public FROM books WHERE id = ?",
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::NotFound("书籍不存在".to_string()))?;

    // 检查权限
    if book.user_id != auth.user_id && !book.is_public {
        return Err(AppError::Forbidden("无权访问该书籍".to_string()));
    }

    // 获取文件内容
    let content = fs::read_to_string(&book.file_path)
        .await
        .map_err(AppError::Io)?;

    // 计算内容长度
    let content_length = content.chars().count();

    // 确保位置有效
    let position = params.position.max(0) as usize;
    if position >= content_length {
        return Err(AppError::BadRequest("位置超出内容范围".to_string()));
    }

    // 计算要返回的内容长度
    let length = params.length.unwrap_or(4000).clamp(100, 10000) as usize;

    // 提取内容
    let end_pos = (position + length).min(content_length);
    let content_slice: String = content
        .chars()
        .skip(position)
        .take(end_pos - position)
        .collect();

    // 构建响应
    let response = BookContentResponse {
        content: content_slice,
        next_position: end_pos as i64,
    };

    Ok(Json(ApiResponse::success(response)))
}

// 跳转到指定章节
async fn jump_to_chapter(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Query(params): Query<JumpToChapterParams>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    // 查询书籍信息
    let book = sqlx::query!("SELECT user_id, is_public FROM books WHERE id = ?", book_id)
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| AppError::NotFound("书籍不存在".to_string()))?;

    // 检查权限
    if book.user_id != auth.user_id && !book.is_public {
        return Err(AppError::Forbidden("无权访问该书籍".to_string()));
    }

    // 查询章节信息
    let chapter = sqlx::query!(
        "SELECT position FROM chapters WHERE id = ? AND book_id = ?",
        params.chapter_id,
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::NotFound("章节不存在".to_string()))?;

    // 返回章节位置
    Ok(Json(ApiResponse::success(serde_json::json!({
        "position": chapter.position
    }))))
}

// 获取公开书籍列表
async fn list_public_books(
    _auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    // 解析分页参数
    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(10);
    let offset = (page - 1) * limit;

    // 获取总数
    let total = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM books WHERE is_public = 1")
        .fetch_one(&pool)
        .await?;

    // 获取公开书籍列表
    let books = sqlx::query!(
        r#"
        SELECT b.id, b.title, b.author, b.created_at, u.username as owner_username
        FROM books b
        JOIN users u ON b.user_id = u.id
        WHERE b.is_public = 1
        ORDER BY b.created_at DESC
        LIMIT ? OFFSET ?
        "#,
        limit,
        offset
    )
    .fetch_all(&pool)
    .await?;

    // 构建响应
    let book_list: Vec<PublicBookListItem> = books
        .into_iter()
        .map(|book| PublicBookListItem {
            book_id: book.id,
            title: book.title,
            author: book.author,
            owner_username: book.owner_username,
            created_at: book.created_at,
        })
        .collect();

    Ok(Json(ApiResponse::success(serde_json::json!({
        "total": total,
        "books": book_list
    }))))
}

// 随机获取公开书籍
async fn get_random_public_books(
    _auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Query(params): Query<RandomPublicParams>,
) -> Result<Json<ApiResponse<serde_json::Value>>, AppError> {
    // 确定要返回的书籍数量
    let count = params.count.unwrap_or(1).clamp(1, 10);

    // 获取所有公开书籍
    let books = sqlx::query!(
        r#"
        SELECT b.id, b.title, b.author, b.created_at, u.username as owner_username
        FROM books b
        JOIN users u ON b.user_id = u.id
        WHERE b.is_public = 1
        "#
    )
    .fetch_all(&pool)
    .await?;

    // 随机选择书籍
    let mut rng = rand::rng();
    let selected_books: Vec<_> = books
        .choose_multiple(&mut rng, count as usize)
        .map(|book| PublicBookListItem {
            book_id: book.id,
            title: book.title.clone(),
            author: book.author.clone(),
            owner_username: book.owner_username.clone(),
            created_at: book.created_at.clone(),
        })
        .collect();

    Ok(Json(ApiResponse::success(serde_json::json!({
        "books": selected_books
    }))))
}
