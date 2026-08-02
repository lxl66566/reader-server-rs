use std::path::PathBuf;

use axum::{
    Json, Router,
    extract::{Path, Query, State, multipart::Multipart},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
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
        Book, BookContentResponse, BookDetailResponse, BookListItem, BookListResponse,
        BookSourceFormat, BookTagsResponse, ChapterResponse, JumpToChapterResponse,
        PublicBookListItem, PublicBookListResponse, RandomPublicBooksResponse, SetBookTagsRequest,
        UpdateBookRequest, UploadBookResponse,
    },
    routes::reading::etag_matches,
    utils::{extract_chapters, generate_uuid, make_etag, parse_epub, sha256_hex},
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

// 搜索查询参数
// scope: 缺省="mine"（仅自己的书）；"public"=公开书库；"all"=两者去重
#[derive(Debug, Deserialize, Default)]
pub struct SearchParams {
    pub q: String,
    pub scope: Option<String>,
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

// 搜索范围枚举：编译期约束，避免运行期字符串匹配出错
#[derive(Debug, Clone, Copy)]
enum SearchScope {
    Mine,
    Public,
    All,
}

impl SearchScope {
    fn parse(s: Option<&str>) -> Result<Self, AppError> {
        match s {
            None | Some("" | "mine") => Ok(SearchScope::Mine),
            Some("public") => Ok(SearchScope::Public),
            Some("all") => Ok(SearchScope::All),
            Some(_) => Err(AppError::InvalidSearchScope),
        }
    }
}

// 书籍路由
pub fn routes() -> Router<crate::app::AppState> {
    // 注意：所有 /{book_id}/... 路径必须放在同一个 Router 中，否则 axum 0.8
    // 的路径树在某些动态+静态混合情况下会返回 422（路由匹配失败）。
    Router::new()
        .route("/upload", post(upload_book))
        .route("/", get(list_books))
        .route("/search", get(search_books))
        .route("/public", get(list_public_books))
        .route("/random_public", get(get_random_public_books))
        .route(
            "/{book_id}",
            get(get_book_detail).put(update_book).delete(delete_book),
        )
        .route("/{book_id}/content", get(get_book_content))
        .route("/{book_id}/jump_to_chapter", get(jump_to_chapter))
        .route(
            "/{book_id}/bookmarks",
            get(crate::routes::annotations::list_bookmarks)
                .post(crate::routes::annotations::create_bookmark),
        )
        .route(
            "/{book_id}/bookmarks/{bookmark_id}",
            delete(crate::routes::annotations::delete_bookmark),
        )
        .route(
            "/{book_id}/annotations",
            get(crate::routes::annotations::list_annotations)
                .post(crate::routes::annotations::create_annotation),
        )
        .route(
            "/{book_id}/annotations/{annotation_id}",
            put(crate::routes::annotations::update_annotation)
                .delete(crate::routes::annotations::delete_annotation),
        )
        .route("/{book_id}/tags", get(get_book_tags).put(set_book_tags))
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

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::FormParseError(e.to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "title" => {
                title =
                    Some(field.text().await.map_err(|e| {
                        AppError::FieldReadError("title".to_string(), e.to_string())
                    })?);
            }
            "author" => {
                author =
                    Some(field.text().await.map_err(|e| {
                        AppError::FieldReadError("author".to_string(), e.to_string())
                    })?);
            }
            "is_public" => {
                let value = field.text().await.map_err(|e| {
                    AppError::FieldReadError("is_public".to_string(), e.to_string())
                })?;
                is_public = value == "true" || value == "1";
            }
            "file" => {
                file_content =
                    Some(field.bytes().await.map_err(|e| {
                        AppError::FieldReadError("file".to_string(), e.to_string())
                    })?);
            }
            _ => {}
        }
    }

    // 验证必要字段
    let title = title.ok_or_else(|| AppError::TitleEmpty)?;
    let file_content = file_content.ok_or_else(|| AppError::FileEmpty)?;

    // 验证文件大小（从配置读取上限）
    if file_content.len() > config.storage.max_file_size {
        return Err(AppError::FileTooLarge);
    }

    // 基于文件内容（magic bytes）检测类型，不依赖客户端扩展名
    let (content, chapters, file_ext_str, source_format) = if let Some(kind) =
        infer::get(&file_content)
    {
        match kind.mime_type() {
            "application/epub+zip" => {
                let result = parse_epub(&file_content)?;
                (result.text_content, result.chapters, "txt", BookSourceFormat::Epub)
            }
            // 其他已知二进制格式（exe、pdf 等）一律拒绝
            _ => return Err(AppError::UnsupportedFileFormat),
        }
    } else {
        // infer 无法识别（通常是纯文本）：尝试 UTF-8 解码
        match std::str::from_utf8(&file_content) {
            Ok(s) => {
                let text = s.to_string();
                let chs = extract_chapters(&text);
                (text, chs, "txt", BookSourceFormat::Txt)
            }
            Err(_) => return Err(AppError::FileInvalidUtf8),
        }
    };

    // 派生元数据：字符数（用于内容边界检查、阅读百分比）+ 内容 hash（去重 / 溯源）
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let total_chars = content.chars().count() as i64;
    let content_hash = sha256_hex(content.as_bytes());

    // 生成唯一文件名
    let file_id = generate_uuid();
    let file_path =
        PathBuf::from(&config.storage.book_dir).join(format!("{file_id}.{file_ext_str}"));
    let file_path_string = file_path.to_string_lossy().into_owned();

    // 先写入文件
    let mut file = fs::File::create(&file_path).await.map_err(AppError::Io)?;
    file.write_all(content.as_bytes())
        .await
        .map_err(AppError::Io)?;
    drop(file); // 确保文件句柄关闭

    // 将书籍信息和章节写入数据库；失败时清理已写入的文件，避免孤儿文件
    let db_result: Result<(i64, Vec<ChapterResponse>), AppError> = async {
        let book_id = sqlx::query!(
            "INSERT INTO books (user_id, title, author, file_path, is_public, total_chars, source_format, content_hash) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            auth.user_id,
            title,
            author,
            file_path_string,
            is_public,
            total_chars,
            source_format,
            content_hash,
        )
        .execute(&pool)
        .await?
        .last_insert_rowid();

        // 批量插入章节（单次 roundtrip），通过 last_insert_rowid 推算所有 id
        let chapter_responses = if chapters.is_empty() {
            Vec::new()
        } else {
            let mut chapter_qb = sqlx::QueryBuilder::new(
                "INSERT INTO chapters (book_id, title, position) ",
            );
            chapter_qb.push_values(chapters.iter(), |mut b, (title, position)| {
                b.push_bind(book_id)
                    .push_bind(title.clone())
                    .push_bind(*position);
            });
            let result = chapter_qb.build().execute(&pool).await?;

            // SQLite 多行 INSERT 的 rowid 连续递增
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
            let count = chapters.len() as i64;
            let last_id = result.last_insert_rowid();
            let first_id = last_id - count + 1;

            chapters
                .into_iter()
                .zip(first_id..=last_id)
                .map(|((title, position), chapter_id)| ChapterResponse {
                    chapter_id,
                    title,
                    position,
                })
                .collect()
        };

        // 创建初始阅读进度
        sqlx::query!(
            "INSERT INTO reading_progress (user_id, book_id) VALUES (?, ?)",
            auth.user_id,
            book_id
        )
        .execute(&pool)
        .await?;

        Ok((book_id, chapter_responses))
    }
    .await;

    let (book_id, chapter_responses) = match db_result {
        Ok(v) => v,
        Err(e) => {
            // 清理已写入的文件
            let _ = fs::remove_file(&file_path).await;
            return Err(e);
        }
    };

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
) -> Result<Json<ApiResponse<BookListResponse>>, AppError> {
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
               b.total_chars, b.description,
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
            total_chars: book.total_chars,
            description: book.description,
        })
        .collect();

    Ok(Json(ApiResponse::success(BookListResponse {
        total,
        books: book_list,
    })))
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
        .ok_or_else(|| AppError::BookNotFound)?;

    // 检查权限
    if book.user_id != auth.user_id {
        // 如果不是书籍所有者，检查书籍是否公开
        if !book.is_public {
            return Err(AppError::BookAccessDenied);
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
        total_chars: book.total_chars,
        description: book.description,
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
        .ok_or_else(|| AppError::BookNotFound)?;

    // 验证权限
    if book.user_id != auth.user_id {
        return Err(AppError::BookModifyDenied);
    }

    // 构建动态更新 SQL（使用 separated 自动处理逗号分隔）
    let mut query_builder: sqlx::QueryBuilder<sqlx::Sqlite> =
        sqlx::QueryBuilder::new("UPDATE books SET ");
    let mut sep = query_builder.separated(", ");

    let mut has_update = false;
    if let Some(title) = &req.title {
        sep.push("title = ").push_bind_unseparated(title.clone());
        has_update = true;
    }
    if let Some(author) = &req.author {
        sep.push("author = ").push_bind_unseparated(author.clone());
        has_update = true;
    }
    if let Some(is_public) = req.is_public {
        sep.push("is_public = ").push_bind_unseparated(is_public);
        has_update = true;
    }
    if let Some(cover_path) = &req.cover_path {
        sep.push("cover_path = ")
            .push_bind_unseparated(cover_path.clone());
        has_update = true;
    }

    if !has_update {
        return Ok(Json(ApiResponse::<()>::message("No updates")));
    }

    query_builder.push(" WHERE id = ").push_bind(book_id);
    query_builder.build().execute(&pool).await?;

    Ok(Json(ApiResponse::<()>::message("Update successful")))
}

// 删除书籍
async fn delete_book(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    State(_config): State<Config>,
    Path(book_id): Path<i64>,
) -> Result<Json<ApiResponse<()>>, AppError> {
    // 在事务内查询书籍信息并验证权限
    let mut tx = pool.begin().await?;
    let book = sqlx::query!("SELECT user_id, file_path FROM books WHERE id = ?", book_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::BookNotFound)?;

    // 验证权限
    if book.user_id != auth.user_id {
        return Err(AppError::BookDeleteDenied);
    }

    let file_path = book.file_path;

    // 先删除数据库记录（外键约束会级联删除章节和阅读进度）
    sqlx::query!("DELETE FROM books WHERE id = ?", book_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    // DB 删除成功后再删文件；文件删除失败只记日志，不影响响应。
    // 孤儿文件比"DB 记录存在但文件丢失"更安全。
    if PathBuf::from(&file_path).exists() {
        if let Err(e) = fs::remove_file(&file_path).await {
            tracing::warn!(error = %e, file_path = %file_path, "Failed to delete book file");
        }
    }

    Ok(Json(ApiResponse::<()>::message("Deleted successfully")))
}

// 获取书籍内容
async fn get_book_content(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Query(params): Query<ContentParams>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    // 查询书籍信息
    let book = sqlx::query!(
        "SELECT user_id, file_path, is_public FROM books WHERE id = ?",
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::BookNotFound)?;

    // 检查权限
    if book.user_id != auth.user_id && !book.is_public {
        return Err(AppError::BookAccessDenied);
    }

    // 获取文件内容
    let content = fs::read_to_string(&book.file_path)
        .await
        .map_err(AppError::Io)?;

    // 计算内容长度
    let content_length = content.chars().count();

    // 确保位置有效
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let position = params.position.max(0) as usize;
    if position >= content_length {
        return Err(AppError::PositionExceedsRange);
    }

    // 计算要返回的内容长度
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let length = params.length.unwrap_or(4000).clamp(100, 10000) as usize;

    // 提取内容
    let end_pos = (position + length).min(content_length);
    let content_slice: String = content
        .chars()
        .skip(position)
        .take(end_pos - position)
        .collect();

    // 构建响应
    let resp_data = BookContentResponse {
        content: content_slice,
        #[allow(clippy::cast_possible_wrap)]
        next_position: end_pos as i64,
    };

    // 基于 book_id + position + 内容片段生成 ETag；同书同位置同长度则命中
    let etag_input = format!("{book_id}:{position}:{length}:{}", resp_data.content);
    let etag = make_etag(&etag_input);

    if let Some(if_none) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && etag_matches(if_none, &etag)
    {
        return Ok(axum::http::StatusCode::NOT_MODIFIED.into_response());
    }

    Ok((
        [(header::ETAG, etag.as_str())],
        Json(ApiResponse::success(resp_data)),
    )
        .into_response())
}

// 跳转到指定章节
async fn jump_to_chapter(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Query(params): Query<JumpToChapterParams>,
) -> Result<Json<ApiResponse<JumpToChapterResponse>>, AppError> {
    // 校验 chapter_id 有效
    if params.chapter_id <= 0 {
        return Err(AppError::ChapterNotFound);
    }

    // 查询书籍信息
    let book = sqlx::query!("SELECT user_id, is_public FROM books WHERE id = ?", book_id)
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| AppError::BookNotFound)?;

    // 检查权限
    if book.user_id != auth.user_id && !book.is_public {
        return Err(AppError::BookAccessDenied);
    }

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
    Ok(Json(ApiResponse::success(JumpToChapterResponse {
        position: chapter.position,
    })))
}

// 获取公开书籍列表
async fn list_public_books(
    _auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ApiResponse<PublicBookListResponse>>, AppError> {
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
        SELECT b.id, b.title, b.author, b.created_at, b.total_chars, b.description, u.username as owner_username
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
            total_chars: book.total_chars,
            description: book.description,
        })
        .collect();

    Ok(Json(ApiResponse::success(PublicBookListResponse {
        total,
        books: book_list,
    })))
}

// 随机获取公开书籍
async fn get_random_public_books(
    _auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Query(params): Query<RandomPublicParams>,
) -> Result<Json<ApiResponse<RandomPublicBooksResponse>>, AppError> {
    // 确定要返回的书籍数量
    let count = params.count.unwrap_or(1).clamp(1, 10);

    // 获取所有公开书籍
    let books = sqlx::query!(
        r#"
        SELECT b.id, b.title, b.author, b.created_at, b.total_chars, b.description, u.username as owner_username
        FROM books b
        JOIN users u ON b.user_id = u.id
        WHERE b.is_public = 1
        "#
    )
    .fetch_all(&pool)
    .await?;

    // 随机选择书籍
    let mut rng = rand::rng();
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let selected_books: Vec<PublicBookListItem> = books
        .sample(&mut rng, count as usize)
        .map(|book| PublicBookListItem {
            book_id: book.id,
            title: book.title.clone(),
            author: book.author.clone(),
            owner_username: book.owner_username.clone(),
            created_at: book.created_at.clone(),
            total_chars: book.total_chars,
            description: book.description.clone(),
        })
        .collect();

    Ok(Json(ApiResponse::success(RandomPublicBooksResponse {
        books: selected_books,
    })))
}

/// 转义 `SQLite` LIKE 的特殊字符（%, _, \），用 ESCAPE '\' 配合
fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    for ch in input.chars() {
        match ch {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

// 搜索结果行（私用 FromRow 结构，避免在闭包里手动取列）
#[derive(sqlx::FromRow)]
struct SearchRow {
    id: i64,
    title: String,
    author: Option<String>,
    is_public: bool,
    created_at: String,
    total_chars: i64,
    description: Option<String>,
    position: Option<i64>,
    reading_time: Option<i64>,
    last_read_at: Option<String>,
}

// 搜索书籍：按 title/author/章节标题 LIKE 模糊匹配
// 不做书内全文索引（FTS），因为后续书籍正文可能不再存 sqlite
async fn search_books(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Query(params): Query<SearchParams>,
) -> Result<Json<ApiResponse<BookListResponse>>, AppError> {
    let q = params.q.trim();
    if q.is_empty() {
        return Err(AppError::SearchQueryEmpty);
    }

    let scope = SearchScope::parse(params.scope.as_deref())?;
    let page = params.page.unwrap_or(1).max(1);
    let limit = params.limit.unwrap_or(10).clamp(1, 50);
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let offset = i64::from((page - 1) * limit);

    // 构造 LIKE 模式：`%query%`，先转义用户输入的特殊字符
    let pattern = format!("%{}%", escape_like(q));

    // 使用 DISTINCT 因为 LEFT JOIN chapters 会让一本书出现多次
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT DISTINCT b.id, b.title, b.author, b.is_public, b.created_at, \
         b.total_chars AS total_chars, b.description AS description, \
         rp.position AS position, rp.reading_time AS reading_time, rp.last_read_at AS last_read_at \
         FROM books b \
         LEFT JOIN reading_progress rp ON rp.book_id = b.id AND rp.user_id = ",
    );
    qb.push_bind(auth.user_id);
    qb.push(" LEFT JOIN chapters c ON c.book_id = b.id WHERE (b.title LIKE ");
    qb.push_bind(&pattern);
    qb.push(" ESCAPE '\\' OR b.author LIKE ");
    qb.push_bind(&pattern);
    qb.push(" ESCAPE '\\' OR c.title LIKE ");
    qb.push_bind(&pattern);
    qb.push(" ESCAPE '\\') AND (");

    push_scope_condition(&mut qb, scope, auth.user_id);
    qb.push(") ORDER BY b.created_at DESC LIMIT ")
        .push_bind(limit);
    qb.push(" OFFSET ").push_bind(offset);

    let rows = qb.build_query_as::<SearchRow>().fetch_all(&pool).await?;

    let book_list: Vec<BookListItem> = rows
        .into_iter()
        .map(|r| BookListItem {
            book_id: r.id,
            title: r.title,
            author: r.author,
            is_public: r.is_public,
            created_at: r.created_at,
            last_read_at: r.last_read_at,
            position: r.position.unwrap_or(0),
            reading_time: r.reading_time.unwrap_or(0),
            total_chars: r.total_chars,
            description: r.description,
        })
        .collect();

    // 单独查询总数，避免 DISTINCT + JOIN 的复杂聚合影响主查询性能
    let mut count_qb = sqlx::QueryBuilder::new(
        "SELECT COUNT(DISTINCT b.id) FROM books b LEFT JOIN chapters c ON c.book_id = b.id WHERE (b.title LIKE ",
    );
    count_qb.push_bind(&pattern);
    count_qb.push(" ESCAPE '\\' OR b.author LIKE ");
    count_qb.push_bind(&pattern);
    count_qb.push(" ESCAPE '\\' OR c.title LIKE ");
    count_qb.push_bind(&pattern);
    count_qb.push(" ESCAPE '\\') AND (");
    push_scope_condition(&mut count_qb, scope, auth.user_id);
    count_qb.push(")");

    let total: i64 = count_qb
        .build_query_scalar::<i64>()
        .fetch_one(&pool)
        .await?;

    Ok(Json(ApiResponse::success(BookListResponse {
        total,
        books: book_list,
    })))
}

/// 把 scope 条件追加到已构造的 QueryBuilder（WHERE 子句的最后一个分组）
fn push_scope_condition(
    qb: &mut sqlx::QueryBuilder<sqlx::Sqlite>,
    scope: SearchScope,
    user_id: i64,
) {
    match scope {
        SearchScope::Mine => {
            qb.push("b.user_id = ").push_bind(user_id);
        }
        SearchScope::Public => {
            qb.push("b.is_public = 1");
        }
        SearchScope::All => {
            qb.push("b.user_id = ")
                .push_bind(user_id)
                .push(" OR b.is_public = 1");
        }
    }
}

// ============ 书籍标签 ============

/// 验证当前用户对书籍有写权限（必须是 owner；公开书不可改标签）
async fn require_book_owner(
    pool: &Pool<Sqlite>,
    auth: &AuthUser,
    book_id: i64,
) -> Result<(), AppError> {
    let owner_id: Option<i64> = sqlx::query_scalar("SELECT user_id FROM books WHERE id = ?")
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
async fn get_book_tags(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
) -> Result<Json<ApiResponse<BookTagsResponse>>, AppError> {
    // 读权限：所有者或公开书
    let book = sqlx::query!("SELECT user_id, is_public FROM books WHERE id = ?", book_id)
        .fetch_optional(&pool)
        .await?
        .ok_or(AppError::BookNotFound)?;
    if book.user_id != auth.user_id && !book.is_public {
        return Err(AppError::BookAccessDenied);
    }

    let tags: Vec<String> = sqlx::query_scalar(
        "SELECT t.name FROM tags t \
         JOIN book_tags bt ON bt.tag_id = t.id \
         WHERE bt.book_id = ? ORDER BY t.name",
    )
    .bind(book_id)
    .fetch_all(&pool)
    .await?;

    Ok(Json(ApiResponse::success(BookTagsResponse { tags })))
}

// 设置书籍标签（覆盖式）：事务内删旧建新；标签字典不存在则自动创建
async fn set_book_tags(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Json(req): Json<SetBookTagsRequest>,
) -> Result<Json<ApiResponse<BookTagsResponse>>, AppError> {
    require_book_owner(&pool, &auth, book_id).await?;

    // 防止标签里夹带超长或空字符串
    let cleaned: Vec<String> = req
        .tags
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.chars().count() <= 32)
        .collect();
    // 去重（保留首次出现顺序）
    let mut seen = std::collections::HashSet::new();
    let tags: Vec<String> = cleaned
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect();

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
