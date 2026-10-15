use std::path::PathBuf;

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State, multipart::Multipart},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use chrono::{SecondsFormat, Utc};
use serde::Deserialize;
use sqlx::{Pool, Sqlite};
use tokio::{fs, io::AsyncWriteExt};
use tokio_util::io::ReaderStream;

use crate::{
    access::{check_book_access, ensure_book_access},
    auth::AuthUser,
    config::Config,
    error::{ApiResponse, AppError},
    limits::{BOOK_DESCRIPTION_MAX_CHARS, BOOK_SEARCH_QUERY_MAX_CHARS},
    models::{
        Book, BookContentResponse, BookDetailResponse, BookFormat, BookListItem, BookListResponse,
        BookTagsResponse, BookTextSearchMatch, BookTextSearchResponse, ChapterResponse,
        JumpToChapterResponse, PatternUpdate, PublicBookListItem, PublicBookListResponse,
        RandomPublicBooksResponse, SetBookTagsRequest, TrashBookListItem, TrashBookListResponse,
        UpdateBookRequest, UploadBookResponse,
    },
    parser::{detect_format, extract_chapters, parse_epub, parse_pdf, validate_chapter_pattern},
    routes::reading::etag_matches,
    utils::{generate_uuid, make_etag, sha256_hex, utf8::Utf8Chunks},
};

// 分页查询参数
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<u32>,
    pub limit: Option<u32>,
}

impl PaginationParams {
    /// 归一化分页参数，返回可直接用于 SQL 的 `(limit, offset)`。
    /// page 最小为 1（避免 `(page - 1)` u32 下溢），limit 钳制在 `[1, max_limit]`
    /// （避免无上限拉取全表）；offset 先转 i64 再乘，避免 u32 乘法溢出。
    pub fn normalize(&self, default_limit: u32, max_limit: u32) -> (u32, i64) {
        let page = self.page.unwrap_or(1).max(1);
        let limit = self.limit.unwrap_or(default_limit).clamp(1, max_limit);
        (limit, i64::from(page - 1) * i64::from(limit))
    }
}

// 获取内容查询参数（仅 txt 格式有意义）
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

// 原文件下载查询参数
#[derive(Debug, Deserialize)]
pub struct FileParams {
    // true 时以 attachment 返回（浏览器保存为文件，文件名取书名）；
    // 缺省 inline（pdf 内联预览，epub.js/pdf.js fetch 渲染不受影响）
    pub download: Option<bool>,
}

// 随机公开书籍查询参数
#[derive(Debug, Deserialize)]
pub struct RandomPublicParams {
    pub count: Option<i64>,
}

// 搜索查询参数（分页复用 PaginationParams，由独立 Query 提取器解析；
// 避免 serde(flatten) 与 urlencoded 的兼容性问题）
// scope: 缺省="mine"（仅自己的书）；"public"=公开书库；"all"=两者去重
#[derive(Debug, Deserialize, Default)]
pub struct SearchParams {
    pub q: String,
    pub scope: Option<String>,
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

// 书架列表查询参数：排序键/方向与格式筛选（独立 Query 提取器，风格随 SearchParams）
#[derive(Debug, Deserialize, Default)]
pub struct BookListParams {
    pub sort: Option<String>,
    pub order: Option<String>,
    pub format: Option<String>,
}

// 书架排序键：last_read=最近阅读（默认，旧行为）；created=创建时间；title/author=元数据
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum BookSortKey {
    #[default]
    LastRead,
    Created,
    Title,
    Author,
}

impl BookSortKey {
    fn parse(s: Option<&str>) -> Result<Self, AppError> {
        match s {
            None | Some("" | "last_read") => Ok(BookSortKey::LastRead),
            Some("created") => Ok(BookSortKey::Created),
            Some("title") => Ok(BookSortKey::Title),
            Some("author") => Ok(BookSortKey::Author),
            Some(_) => Err(AppError::InvalidBookSort),
        }
    }

    /// ORDER BY 片段：排序键 + `b.id` 兜底保证并列时顺序稳定（created_at 同精度
    /// 内频繁并列，无兜底时顺序不定）。last_read 保持旧行为：未读（NULL）恒排
    /// 最后，键后按创建时间排序。片段全部为编译期常量（match 全覆盖枚举分支），
    /// 不含任何用户输入，无 SQL 注入面；与 QueryBuilder 的 push_bind 组合使用
    fn order_by(self, desc: bool) -> &'static str {
        match (self, desc) {
            (BookSortKey::LastRead, true) => {
                "rp.last_read_at DESC NULLS LAST, b.created_at DESC, b.id DESC"
            },
            (BookSortKey::LastRead, false) => {
                "rp.last_read_at ASC NULLS LAST, b.created_at ASC, b.id ASC"
            },
            (BookSortKey::Created, true) => "b.created_at DESC, b.id DESC",
            (BookSortKey::Created, false) => "b.created_at ASC, b.id ASC",
            // COLLATE NOCASE：ASCII 大小写不敏感；中文排序精度不作要求。
            // author 用 COALESCE 归一 NULL，避免 NULL 在 ASC/DESC 下落位不一致
            (BookSortKey::Title, true) => "b.title COLLATE NOCASE DESC, b.id DESC",
            (BookSortKey::Title, false) => "b.title COLLATE NOCASE ASC, b.id ASC",
            (BookSortKey::Author, true) => "COALESCE(b.author, '') COLLATE NOCASE DESC, b.id DESC",
            (BookSortKey::Author, false) => "COALESCE(b.author, '') COLLATE NOCASE ASC, b.id ASC",
        }
    }
}

// 排序方向：desc=默认（旧行为）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum SortDirection {
    Asc,
    #[default]
    Desc,
}

impl SortDirection {
    fn parse(s: Option<&str>) -> Result<Self, AppError> {
        match s {
            None | Some("" | "desc") => Ok(SortDirection::Desc),
            Some("asc") => Ok(SortDirection::Asc),
            Some(_) => Err(AppError::InvalidSortOrder),
        }
    }
}

// 格式筛选：None = 不过滤
fn parse_format_filter(s: Option<&str>) -> Result<Option<BookFormat>, AppError> {
    match s {
        None | Some("") => Ok(None),
        Some("txt") => Ok(Some(BookFormat::Txt)),
        Some("epub") => Ok(Some(BookFormat::Epub)),
        Some("pdf") => Ok(Some(BookFormat::Pdf)),
        Some(_) => Err(AppError::InvalidFormatFilter),
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
        // 回收站列表（仅本人的已软删书籍）
        .route("/trash", get(list_trash))
        .route(
            "/{book_id}",
            get(get_book_detail).put(update_book).delete(delete_book),
        )
        // 从回收站恢复
        .route("/{book_id}/restore", post(restore_book))
        // 彻底删除（复用级联删除 + 文件清理，不可恢复）
        .route("/{book_id}/purge", delete(purge_book))
        // 原文件下载：epub / pdf 客户端渲染时使用；txt 也可用
        .route("/{book_id}/file", get(download_book_file))
        // 内容切片：仅 txt 有意义；epub/pdf 走 /file
        .route("/{book_id}/content", get(get_book_content))
        .route("/{book_id}/jump_to_chapter", get(jump_to_chapter))
        // 书内全文搜索：仅 txt 有意义；epub/pdf 沿用 content 的错误语义
        .route("/{book_id}/search", get(search_in_book))
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
/// 映射 multipart 解析错误：超过请求体上限时返回 413，其余按原有错误处理
pub(crate) fn map_multipart_err(
    e: &axum::extract::multipart::MultipartError,
    other: impl FnOnce(String) -> AppError,
) -> AppError {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        AppError::RequestBodyTooLarge
    } else {
        other(e.to_string())
    }
}

/// 标签净化：trim、去空、长度限制（32 字符）、去重（保留首次出现顺序）。
/// set_book_tags 与用户数据导入共用同一规则
pub(crate) fn clean_tags(tags: impl IntoIterator<Item = String>) -> Vec<String> {
    let cleaned: Vec<String> = tags
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.chars().count() <= 32)
        .collect();
    let mut seen = std::collections::HashSet::new();
    cleaned
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

/// 单本书标签数量上限（与 [`clean_tags`] 的单标签 32 字符限制同值）。
/// 对齐 WS 协议侧的输入上限防滥用策略；WS 侧（`src/ws/protocol.rs`）当前
/// 未单独限制 tags 数量，若后续补充对应常量，两处应保持一致
pub(crate) const TAGS_MAX_COUNT: usize = 32;

/// 上传解析的 CPU 密集产物：格式、统计、章节、元数据与内容哈希
struct ParsedUpload {
    format: BookFormat,
    total_chars: i64,
    total_pages: i64,
    chapters: Vec<(String, i64)>,
    title: String,
    author: Option<String>,
    /// 简介最终值：用户表单输入优先，其次格式内嵌元数据（epub）
    description: Option<String>,
    content_hash: String,
}

/// 格式分支派生的元数据暂存：统计值、章节表、标题/作者/简介
type ParsedMetadata = (
    i64,
    i64,
    Vec<(String, i64)>,
    String,
    Option<String>,
    Option<String>,
);

/// 同步解析上传文件：格式识别（全文 UTF-8 校验）、元数据派生、章节提取
/// （逐行正则 / 全书 HTML 解析 / PDF 解析）与 sha256，均为 CPU 密集操作，
/// 调用方必须置于 `spawn_blocking`（项目约定，参照 user_data.rs）
fn parse_upload_sync(
    content: &[u8],
    title: String,
    author: Option<String>,
    description: Option<String>,
    chapter_pattern: Option<&str>,
    file_name: Option<&str>,
) -> Result<ParsedUpload, AppError> {
    // 简介归一化：trim，纯空白 = 未提供；超限入库前拒绝
    let description = description
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());
    if description
        .as_ref()
        .is_some_and(|d| d.chars().count() > BOOK_DESCRIPTION_MAX_CHARS)
    {
        return Err(AppError::FieldTooLarge("description"));
    }

    // 检测格式（同时校验内容签名，拒绝未知/二进制格式）
    let format = detect_format(content)?;

    // 章节 regex 仅对 txt 有意义：非 txt 携带非空值拒绝
    if chapter_pattern.is_some() && format != BookFormat::Txt {
        return Err(AppError::ChapterPatternNotSupported);
    }
    validate_chapter_pattern(chapter_pattern.unwrap_or(""))?;

    // 根据格式派生元数据 + 提取章节
    let (total_chars, total_pages, chapters, title, author, description): ParsedMetadata =
        match format {
            BookFormat::Txt => {
                let text = std::str::from_utf8(content).map_err(|_| AppError::FileInvalidUtf8)?;
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                let tc = text.chars().count() as i64;
                let chs = extract_chapters(text, chapter_pattern)?;
                // txt 元数据全部来自用户输入
                (tc, 0i64, chs, title, author, description)
            },
            BookFormat::Epub => {
                let parsed = parse_epub(content)?;
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                let tc = parsed.text_content.chars().count() as i64;
                // 章节：用 spine_index 作为 position（前端按 spine 渲染）
                let chs: Vec<(String, i64)> = parsed
                    .chapters
                    .iter()
                    .map(|c| (c.title.clone(), c.spine_index))
                    .collect();
                // EPUB 解析出的 title/author/description 优先级低于用户上传时的显式输入
                let title = if title.trim().is_empty() {
                    parsed.title.unwrap_or(title)
                } else {
                    title
                };
                // 表单显式提供（归一化后非空）则覆盖元数据提取值
                let description = description.or(parsed.description);
                (tc, 0i64, chs, title, author.or(parsed.author), description)
            },
            BookFormat::Pdf => {
                let parsed = parse_pdf(content)?;
                // PDF 不做章节提取（暂不解析 outline），前端用页码导航
                let title = if title.trim().is_empty() {
                    parsed.title.unwrap_or(title)
                } else {
                    title
                };
                (
                    0i64,
                    parsed.num_pages,
                    Vec::new(),
                    title,
                    author.or(parsed.author),
                    description,
                )
            },
        };

    // 标题兜底：用户输入（trim 非空）> 格式元数据（epub/pdf 已并入）
    // > 上传文件名（去扩展名）；全部为空则拒绝入库
    let title = if title.trim().is_empty() {
        file_name
            .and_then(|name| std::path::Path::new(name).file_stem())
            .and_then(std::ffi::OsStr::to_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or(AppError::TitleEmpty)?
            .to_string()
    } else {
        title
    };

    // 文件 hash（用原文件字节，不依赖格式）
    Ok(ParsedUpload {
        format,
        total_chars,
        total_pages,
        chapters,
        title,
        author,
        description,
        content_hash: sha256_hex(content),
    })
}

async fn upload_book(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    State(config): State<Config>,
    mut multipart: Multipart,
) -> Result<Json<ApiResponse<UploadBookResponse>>, AppError> {
    let mut is_public = false;

    // 文件流式落盘到临时路径（与 book_dir 同目录，rename 不会跨文件系统）：
    // 接收侧不再把整个文件缓冲进内存，内存占用与上传体积无关
    let file_id = generate_uuid();
    let temp_path = PathBuf::from(&config.storage.book_dir).join(format!("{file_id}.part"));

    // multipart 接收 → 阻塞解析 → rename 为正式文件名；无论成败，
    // 收尾统一清理临时文件（成功路径文件已被 rename 走，删除为 NotFound noop）
    let result: Result<(ParsedUpload, PathBuf, Option<String>), AppError> = async {
        // 解析 multipart 表单字段；title 允许缺省（后续按 元数据/文件名 兜底）
        let mut title = None;
        let mut author = None;
        // 可选简介（epub/pdf 缺省时回退内嵌元数据）
        let mut description = None;
        // 上传文件名（用于标题兜底）
        let mut file_name: Option<String> = None;
        // 自定义章节切分正则（仅 txt 有意义）
        let mut chapter_pattern: Option<String> = None;
        // 0 字节文件会成为“不可读之书”（/content 恒 400），直接拒绝
        let mut have_file = false;

        while let Some(mut field) = multipart
            .next_field()
            .await
            .map_err(|e| map_multipart_err(&e, AppError::FormParseError))?
        {
            let name = field.name().unwrap_or("").to_string();

            match name.as_str() {
                "title" => {
                    title = Some(field.text().await.map_err(|e| {
                        AppError::FieldReadError("title".to_string(), e.to_string())
                    })?);
                },
                "author" => {
                    author = Some(field.text().await.map_err(|e| {
                        AppError::FieldReadError("author".to_string(), e.to_string())
                    })?);
                },
                "description" => {
                    description = Some(field.text().await.map_err(|e| {
                        AppError::FieldReadError("description".to_string(), e.to_string())
                    })?);
                },
                "is_public" => {
                    let value = field.text().await.map_err(|e| {
                        AppError::FieldReadError("is_public".to_string(), e.to_string())
                    })?;
                    is_public = value == "true" || value == "1";
                },
                "chapter_pattern" => {
                    chapter_pattern = Some(field.text().await.map_err(|e| {
                        AppError::FieldReadError("chapter_pattern".to_string(), e.to_string())
                    })?);
                },
                "file" => {
                    file_name = field.file_name().map(str::to_string);
                    let mut file = fs::File::create(&temp_path).await.map_err(AppError::Io)?;
                    let mut written: usize = 0;
                    while let Some(chunk) = field.chunk().await.map_err(|e| {
                        map_multipart_err(&e, |s| AppError::FieldReadError("file".to_string(), s))
                    })? {
                        written += chunk.len();
                        // 边收边限大小：超限立即中止，不等整个文件落盘
                        if written > config.storage.max_file_size {
                            return Err(AppError::FileTooLarge);
                        }
                        file.write_all(&chunk).await.map_err(AppError::Io)?;
                    }
                    // flush 落盘后再交给阻塞线程，避免读到截断文件
                    file.flush().await.map_err(AppError::Io)?;
                    drop(file);
                    have_file = written > 0;
                },
                _ => {},
            }
        }

        if !have_file {
            return Err(AppError::FileEmpty);
        }

        // 章节 regex 归一化：空 = 默认规则（入库为 NULL）
        let pattern = chapter_pattern
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());

        // CPU 密集段（格式识别 / 解析 / 章节提取 / 哈希）整体移入阻塞线程池，
        // 避免并发上传拖慢所有请求；阻塞内按需读入整文件，处理期峰值
        // ≈ 1×文件大小 + 解析产物，受 max_file_size 约束（解析器均以 &[u8] 为输入，
        // 流式化收益有限，暂不重构）
        let parse_path = temp_path.clone();
        // clone 一份小字符串交给 'static 闭包；pattern 本体随后入库
        let pattern_for_task = pattern.clone();
        let parsed = tokio::task::spawn_blocking(move || -> Result<ParsedUpload, AppError> {
            let content = std::fs::read(&parse_path).map_err(AppError::Io)?;
            parse_upload_sync(
                &content,
                title.unwrap_or_default(),
                author,
                description,
                pattern_for_task.as_deref(),
                file_name.as_deref(),
            )
        })
        .await
        .map_err(|e| AppError::Internal(format!("upload parse task failed: {e}")))??;

        // 解析成功：临时文件转为正式文件名（保留原格式扩展名）
        let file_path = PathBuf::from(&config.storage.book_dir)
            .join(format!("{file_id}.{}", parsed.format.ext()));
        fs::rename(&temp_path, &file_path)
            .await
            .map_err(AppError::Io)?;

        Ok((parsed, file_path, pattern))
    }
    .await;

    let _ = fs::remove_file(&temp_path).await;
    let (parsed, file_path, chapter_pattern) = result?;
    let file_path_string = file_path.to_string_lossy().into_owned();

    // 将书籍信息和章节写入数据库；失败时清理已写入的文件，避免孤儿文件。
    // books + chapters + reading_progress 放进同一事务：任一步失败整体回滚，
    // 避免残留的孤儿 books 行（参照 delete_book 的事务用法）
    let db_result: Result<(i64, Vec<ChapterResponse>), AppError> = async {
        let mut tx = pool.begin().await?;

        let book_id = sqlx::query!(
            "INSERT INTO books (user_id, title, author, description, file_path, is_public, \
             total_chars, total_pages, format, content_hash, chapter_pattern) VALUES (?, ?, ?, ?, \
             ?, ?, ?, ?, ?, ?, ?)",
            auth.user_id,
            parsed.title,
            parsed.author,
            parsed.description,
            file_path_string,
            is_public,
            parsed.total_chars,
            parsed.total_pages,
            parsed.format,
            parsed.content_hash,
            chapter_pattern,
        )
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();

        // 批量插入章节（单次 roundtrip），通过 last_insert_rowid 推算所有 id
        let chapter_responses = if parsed.chapters.is_empty() {
            Vec::new()
        } else {
            let mut chapter_qb =
                sqlx::QueryBuilder::new("INSERT INTO chapters (book_id, title, position) ");
            chapter_qb.push_values(parsed.chapters.iter(), |mut b, (title, position)| {
                b.push_bind(book_id)
                    .push_bind(title.clone())
                    .push_bind(*position);
            });
            let result = chapter_qb.build().execute(&mut *tx).await?;

            // SQLite 多行 INSERT 的 rowid 连续递增
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_wrap)]
            let count = parsed.chapters.len() as i64;
            let last_id = result.last_insert_rowid();
            let first_id = last_id - count + 1;

            parsed
                .chapters
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
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok((book_id, chapter_responses))
    }
    .await;

    let (book_id, chapter_responses) = match db_result {
        Ok(v) => v,
        Err(e) => {
            // 清理已写入的文件
            let _ = fs::remove_file(&file_path).await;
            return Err(e);
        },
    };

    // 返回响应
    let response = UploadBookResponse {
        book_id,
        title: parsed.title,
        author: parsed.author,
        format: parsed.format,
        total_chars: parsed.total_chars,
        total_pages: parsed.total_pages,
        chapters: chapter_responses,
    };

    Ok(Json(ApiResponse::success(response)))
}

// 获取用户书籍列表
async fn list_books(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Query(pagination): Query<PaginationParams>,
    Query(params): Query<BookListParams>,
) -> Result<Json<ApiResponse<BookListResponse>>, AppError> {
    // 归一化分页参数：page 最小为 1，limit 上限 50
    let (limit, offset) = pagination.normalize(10, 50);
    let sort = BookSortKey::parse(params.sort.as_deref())?;
    let direction = SortDirection::parse(params.order.as_deref())?;
    let format = parse_format_filter(params.format.as_deref())?;

    // total 必须与列表共用同一筛选条件：有格式过滤时统计过滤后的总数；
    // 回收站中的书不计入书架
    let total = if format.is_some() {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM books WHERE user_id = ? AND deleted_at IS NULL AND format = ?",
        )
        .bind(auth.user_id)
        .bind(format)
        .fetch_one(&pool)
        .await?
    } else {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM books WHERE user_id = ? AND deleted_at IS NULL",
        )
        .bind(auth.user_id)
        .fetch_one(&pool)
        .await?
    };

    // 动态排序键 + 可选格式筛选：外部输入一律 push_bind；ORDER BY 片段来自
    // BookSortKey::order_by 的编译期常量映射（见其注释），非用户输入
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT b.id, b.title, b.author, b.is_public, b.created_at, b.total_chars AS total_chars, \
         b.total_pages AS total_pages, b.description AS description, b.format AS format, \
         rp.locator AS locator, rp.percent AS percent, rp.reading_time AS reading_time, \
         rp.last_read_at AS last_read_at FROM books b LEFT JOIN reading_progress rp ON b.id = \
         rp.book_id AND rp.user_id = ",
    );
    qb.push_bind(auth.user_id)
        .push(" WHERE b.user_id = ")
        .push_bind(auth.user_id)
        .push(" AND b.deleted_at IS NULL");
    if let Some(format) = format {
        qb.push(" AND b.format = ").push_bind(format);
    }
    qb.push(" ORDER BY ")
        .push(sort.order_by(direction == SortDirection::Desc));
    qb.push(" LIMIT ").push_bind(limit);
    qb.push(" OFFSET ").push_bind(offset);

    let books = qb.build_query_as::<BookListRow>().fetch_all(&pool).await?;

    // 构建响应
    let book_list: Vec<BookListItem> = books
        .into_iter()
        .map(|book| BookListItem {
            book_id: book.id,
            title: book.title,
            author: book.author,
            is_public: book.is_public,
            format: book.format,
            created_at: book.created_at,
            last_read_at: book.last_read_at,
            locator: book.locator,
            percent: book.percent.unwrap_or(0.0),
            reading_time: book.reading_time.unwrap_or(0),
            total_chars: book.total_chars,
            total_pages: book.total_pages,
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
    // 查询书籍信息（已移入回收站的书按不存在处理）
    let book = sqlx::query_as::<_, Book>("SELECT * FROM books WHERE id = ? AND deleted_at IS NULL")
        .bind(book_id)
        .fetch_optional(&pool)
        .await?
        .ok_or_else(|| AppError::BookNotFound)?;

    // 检查权限（所有者或公开书）
    ensure_book_access(auth.user_id, book.user_id, book.is_public)?;

    // 查询章节信息
    let chapters = sqlx::query!(
        "SELECT id, title, position FROM chapters WHERE book_id = ? ORDER BY position",
        book_id
    )
    .fetch_all(&pool)
    .await?;

    // 查询阅读进度
    let progress = sqlx::query!(
        r#"SELECT locator, percent, reading_time, last_read_at FROM reading_progress 
         WHERE user_id = ? AND book_id = ?"#,
        auth.user_id,
        book_id
    )
    .fetch_optional(&pool)
    .await?;

    // 如果没有阅读进度，创建一个
    let (locator, percent, reading_time, last_read_at) = if let Some(p) = progress {
        (Some(p.locator), p.percent, p.reading_time, p.last_read_at)
    } else {
        // 如果是公开书籍，为当前用户创建进度记录
        // INSERT OR IGNORE：并发请求会同时进入此分支，裸 INSERT 会让后到的请求撞
        // UNIQUE(user_id, book_id) 变 500，忽略冲突即可（进度行内容一致）
        if book.user_id != auth.user_id {
            sqlx::query!(
                "INSERT OR IGNORE INTO reading_progress (user_id, book_id) VALUES (?, ?)",
                auth.user_id,
                book_id
            )
            .execute(&pool)
            .await?;
        }
        (None, 0.0, 0, None)
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
        format: book.format,
        created_at: book.created_at,
        last_read_at,
        locator,
        percent,
        reading_time,
        total_chars: book.total_chars,
        total_pages: book.total_pages,
        description: book.description,
        is_owner: book.user_id == auth.user_id,
        chapter_pattern: book.chapter_pattern,
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
    // 检查书籍是否存在并属于当前用户（回收站中的书不可编辑）
    let book = sqlx::query!(
        r#"SELECT user_id, format AS "format!: BookFormat", file_path, chapter_pattern FROM books WHERE id = ? AND deleted_at IS NULL"#,
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::BookNotFound)?;

    // 验证权限
    if book.user_id != auth.user_id {
        return Err(AppError::BookModifyDenied);
    }

    // 章节 regex 仅对 txt 有意义：非空值拒绝；null/空串（Reset）对非 txt 无意义，视为未提供
    let pattern_update = match req.chapter_pattern {
        PatternUpdate::Set(_) if book.format != BookFormat::Txt => {
            return Err(AppError::ChapterPatternNotSupported);
        },
        PatternUpdate::Reset if book.format != BookFormat::Txt => PatternUpdate::Unchanged,
        other => other,
    };

    // 与库中现值比较：只有实际变化才重切章节（避免无谓的全文件重提）
    let stored_pattern = book
        .chapter_pattern
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    let pattern_changed = match &pattern_update {
        PatternUpdate::Set(p) => stored_pattern != Some(p.as_str()),
        PatternUpdate::Reset => stored_pattern.is_some(),
        PatternUpdate::Unchanged => false,
    };

    // 简介三态归一：字段缺省 = 不修改（None）；空/纯空白 = 清空（Some(None)）；
    // 非空 = 更新（Some(Some(trimmed))）。超限拒绝（400）
    let description_update = match req.description.as_deref() {
        None => None,
        Some(text) => {
            if text.chars().count() > BOOK_DESCRIPTION_MAX_CHARS {
                return Err(AppError::FieldTooLarge("description"));
            }
            let trimmed = text.trim();
            Some(if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            })
        },
    };

    let has_update = req.title.is_some()
        || req.author.is_some()
        || req.is_public.is_some()
        || req.cover_path.is_some()
        || description_update.is_some()
        || pattern_changed;

    if !has_update {
        return Ok(Json(ApiResponse::<()>::message("No updates")));
    }

    // 变更时从磁盘原文件重提章节；文件可能被外部清理，沿用下载路径的 404 语义
    let new_chapters: Option<Vec<(String, i64)>> = if pattern_changed {
        let path = PathBuf::from(&book.file_path);
        if !path.exists() {
            tracing::error!(book_id, file_path = %book.file_path, "book file missing on disk");
            return Err(AppError::BookNotFound);
        }
        let pattern = match &pattern_update {
            PatternUpdate::Set(p) => Some(p.clone()),
            _ => None,
        };
        // 读全文 + 逐行正则切分为 CPU 密集操作，移入阻塞线程池（参照 user_data.rs）；
        // 上传时已保证 UTF-8，此处失败即文件被外部篡改，沿用 Io 错误语义
        Some(
            tokio::task::spawn_blocking(move || -> Result<Vec<(String, i64)>, AppError> {
                let text = std::fs::read_to_string(&path).map_err(AppError::Io)?;
                extract_chapters(&text, pattern.as_deref())
            })
            .await
            .map_err(|e| AppError::Internal(format!("chapter re-split task failed: {e}")))??,
        )
    } else {
        None
    };

    // 元数据更新与章节重切放同一事务，避免出现“规则已更新但章节未变”的中间态
    let mut tx = pool.begin().await?;

    apply_book_field_updates(
        &mut tx,
        book_id,
        req.title.as_deref(),
        req.author.as_deref(),
        req.is_public,
        req.cover_path.as_deref(),
    )
    .await?;

    // 简介独立更新：apply_book_field_updates 与管理员编辑路径共用，签名不扩散；
    // 绑定 Option<String>，None 即清空为 NULL
    if let Some(value) = &description_update {
        sqlx::query!(
            "UPDATE books SET description = ? WHERE id = ?",
            value,
            book_id
        )
        .execute(&mut *tx)
        .await?;
    }

    if pattern_changed {
        // 恢复默认时写 NULL
        let stored: Option<String> = match &pattern_update {
            PatternUpdate::Set(p) => Some(p.clone()),
            _ => None,
        };
        sqlx::query!(
            "UPDATE books SET chapter_pattern = ? WHERE id = ?",
            stored,
            book_id
        )
        .execute(&mut *tx)
        .await?;
    }

    if let Some(chapters) = new_chapters {
        sqlx::query!("DELETE FROM chapters WHERE book_id = ?", book_id)
            .execute(&mut *tx)
            .await?;
        if !chapters.is_empty() {
            let mut chapter_qb =
                sqlx::QueryBuilder::new("INSERT INTO chapters (book_id, title, position) ");
            chapter_qb.push_values(chapters.iter(), |mut b, (title, position)| {
                b.push_bind(book_id)
                    .push_bind(title.clone())
                    .push_bind(*position);
            });
            chapter_qb.build().execute(&mut *tx).await?;
        }
    }

    tx.commit().await?;

    Ok(Json(ApiResponse::<()>::message("Update successful")))
}

// 删除书籍（软删除）：写 deleted_at 移入回收站，进度/批注等数据原样保留，
// 可经 /restore 恢复；彻底删除走 /purge 或管理员删除
async fn delete_book(
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
async fn list_trash(
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
async fn restore_book(
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
async fn purge_book(
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

/// 动态更新书籍元数据字段（仅 SET 提供的列；全为 None 时不执行 SQL）。
/// 用户侧 PUT /books/:id 与管理员侧编辑共用同一更新路径
pub(crate) async fn apply_book_field_updates(
    tx: &mut sqlx::SqliteConnection,
    book_id: i64,
    title: Option<&str>,
    author: Option<&str>,
    is_public: Option<bool>,
    cover_path: Option<&str>,
) -> Result<(), AppError> {
    if title.is_none() && author.is_none() && is_public.is_none() && cover_path.is_none() {
        return Ok(());
    }
    // 构建动态更新 SQL（使用 separated 自动处理逗号分隔）
    let mut query_builder: sqlx::QueryBuilder<Sqlite> =
        sqlx::QueryBuilder::new("UPDATE books SET ");
    let mut sep = query_builder.separated(", ");
    if let Some(title) = title {
        sep.push("title = ")
            .push_bind_unseparated(title.to_string());
    }
    if let Some(author) = author {
        sep.push("author = ")
            .push_bind_unseparated(author.to_string());
    }
    if let Some(is_public) = is_public {
        sep.push("is_public = ").push_bind_unseparated(is_public);
    }
    if let Some(cover_path) = cover_path {
        sep.push("cover_path = ")
            .push_bind_unseparated(cover_path.to_string());
    }
    query_builder.push(" WHERE id = ").push_bind(book_id);
    query_builder.build().execute(tx).await?;
    Ok(())
}

/// 事务内删除书籍 DB 行，返回 `(owner_user_id, 磁盘文件路径)`；外键 ON DELETE
/// CASCADE 级联清理章节/标签关联/书签/批注/阅读进度/每日统计。
/// 磁盘文件由调用方在事务提交成功后删除（孤儿文件比"DB 行存在但文件丢失"更安全）
pub(crate) async fn delete_book_cascade(
    tx: &mut sqlx::SqliteConnection,
    book_id: i64,
) -> Result<(i64, String), AppError> {
    let book = sqlx::query!("SELECT user_id, file_path FROM books WHERE id = ?", book_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::BookNotFound)?;
    sqlx::query!("DELETE FROM books WHERE id = ?", book_id)
        .execute(&mut *tx)
        .await?;
    Ok((book.user_id, book.file_path))
}

/// 删除书籍磁盘文件（best-effort）：文件已不存在则跳过，失败仅记日志不影响响应
pub(crate) async fn remove_book_file(file_path: &str) {
    if PathBuf::from(file_path).exists()
        && let Err(e) = fs::remove_file(file_path).await
    {
        tracing::warn!(error = %e, file_path, "Failed to delete book file");
    }
}

/// 下载书籍原始文件
///
/// 用途：
/// - epub / pdf：前端 fetch 后用 epub.js / pdf.js 客户端渲染（缺省 inline）
/// - 下载到本地：`?download=true` 时以 attachment 返回，文件名取书名
/// - txt：可选，前端通常走 /content 分页拉取
///
/// 响应：
/// - 200 + 文件字节流（带 Content-Type、Content-Disposition、ETag）
/// - 客户端可带 If-None-Match 命中 304
///
/// 不实现 HTTP Range：浏览器/CDN 自动处理缓冲；如需 Range 后续可加。
async fn download_book_file(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Query(params): Query<FileParams>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let book = sqlx::query!(
        r#"SELECT user_id, title, author, file_path, is_public, content_hash, format AS "format!: BookFormat" FROM books WHERE id = ? AND deleted_at IS NULL"#,
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::BookNotFound)?;

    // 权限：所有者或公开书
    ensure_book_access(auth.user_id, book.user_id, book.is_public)?;

    // 文件可能被外部清理；缺失时返回 404 而不是 500
    let path = PathBuf::from(&book.file_path);
    if !path.exists() {
        tracing::error!(book_id, file_path = %book.file_path, "book file missing on disk");
        return Err(AppError::BookNotFound);
    }

    // ETag 基于内容 hash（上传时计算并持久化），稳定可缓存；
    // 命中 304 时无需打开文件
    let etag = format!("\"{}\"", book.content_hash);
    if let Some(if_none) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && etag_matches(if_none, &etag)
    {
        return Ok(StatusCode::NOT_MODIFIED.into_response());
    }

    let format = book.format;
    let disposition = build_content_disposition(
        params.download.unwrap_or(false),
        &book.title,
        book.author.as_deref(),
        book_id,
        format,
    );

    // 流式发送，避免大文件整本读入内存
    let file = fs::File::open(&path).await.map_err(AppError::Io)?;
    let content_length = file.metadata().await.map_err(AppError::Io)?.len();
    let stream = ReaderStream::new(file);

    Response::builder()
        .header(header::CONTENT_TYPE, format.mime_type())
        .header(header::CONTENT_LENGTH, content_length)
        .header(header::ETAG, etag.as_str())
        .header(header::CONTENT_DISPOSITION, disposition)
        .header(header::CACHE_CONTROL, "private, no-cache")
        .body(Body::from_stream(stream))
        .map_err(|e| AppError::Internal(format!("failed to build download response: {e}")))
}

/// filename* 按 RFC 5987 编码时保留的安全字符（字母数字之外）
const FILENAME_STAR_SAFE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// 构造 Content-Disposition。
/// inline：浏览器内联展示（pdf 可直接预览）；
/// attachment：保存为文件，文件名为「书名 - 作者」（作者缺省/空白时仅书名）——
/// filename* 用 RFC 5987 百分号编码（支持任意 Unicode），filename 回退值仅保留
/// ASCII 安全字符，纯中文会退化为 book_{id}.{ext}。
fn build_content_disposition(
    attachment: bool,
    title: &str,
    author: Option<&str>,
    book_id: i64,
    format: BookFormat,
) -> String {
    let kind = if attachment {
        "attachment"
    } else {
        "inline"
    };
    let ext = format.ext();
    let stem = match author.map(str::trim).filter(|a| !a.is_empty()) {
        Some(a) => format!("{title} - {a}"),
        None => title.to_string(),
    };
    // quoted-string 内不允许 " 与 \，控制字符也不可行；空格等其余 ASCII 均合法。
    // 纯非 ASCII 的 stem（如中文书名）过滤后只剩 " - " 之类的分隔残片，无意义，
    // 要求至少含一个字母/数字，否则回退 book_{id}
    let fallback_stem: String = stem
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control() && *c != '"' && *c != '\\')
        .collect();
    let fallback = if fallback_stem.chars().any(|c| c.is_ascii_alphanumeric()) {
        format!("{fallback_stem}.{ext}")
    } else {
        format!("book_{book_id}.{ext}")
    };
    let full_name = format!("{stem}.{ext}");
    let encoded = percent_encoding::percent_encode(full_name.as_bytes(), FILENAME_STAR_SAFE);
    format!("{kind}; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

// 获取书籍内容（仅 txt 格式）
async fn get_book_content(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Query(params): Query<ContentParams>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    // 查询书籍信息（回收站中的书不可读）
    let book = sqlx::query!(
        r#"SELECT user_id, file_path, is_public, total_chars, format AS "format!: BookFormat" FROM books WHERE id = ? AND deleted_at IS NULL"#,
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| AppError::BookNotFound)?;

    // 检查权限（所有者或公开书）
    ensure_book_access(auth.user_id, book.user_id, book.is_public)?;

    // 仅 txt 支持；epub/pdf 应该走 /file 下载原文件后客户端渲染
    if book.format != BookFormat::Txt {
        return Err(AppError::ContentEndpointNotSupported);
    }

    let content_length = book.total_chars.max(0);

    // 确保位置有效
    if params.position < 0 || params.position >= content_length {
        return Err(AppError::PositionExceedsRange);
    }
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let position = params.position as usize;

    // 计算要返回的内容长度
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let length = params.length.unwrap_or(4000).clamp(100, 10000) as usize;

    // 计算结束位置（基于已持久化的全文字符数，无需读全文）
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let end_pos = (position + length).min(content_length as usize);

    // 流式读取：避免大书全量进内存
    let content_slice = read_char_slice(&book.file_path, position, end_pos - position).await?;

    // 构建响应
    #[allow(clippy::cast_possible_wrap)]
    let resp_data = BookContentResponse {
        content: content_slice,
        next_position: end_pos as i64,
    };

    // 基于 book_id + position + length + 内容片段生成 ETag；同书同位置同长度则命中
    let etag_input = format!("{book_id}:{position}:{length}:{}", resp_data.content);
    let etag = make_etag(&etag_input);

    if let Some(if_none) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && etag_matches(if_none, &etag)
    {
        return Ok(StatusCode::NOT_MODIFIED.into_response());
    }

    Ok((
        [(header::ETAG, etag.as_str())],
        Json(ApiResponse::success(resp_data)),
    )
        .into_response())
}

/// 从文件中按字符偏移流式读取一段文本。
///
/// 实现要点：[`Utf8Chunks`] 保证每块在字符边界上结束，避免多字节字符被块边界切开。
async fn read_char_slice(
    path: &str,
    char_offset: usize,
    char_count: usize,
) -> Result<String, AppError> {
    let file = fs::File::open(path).await.map_err(AppError::Io)?;
    let mut chunks = Utf8Chunks::new(file);

    let mut collected = String::with_capacity(char_count * 4);
    let mut skipped = 0usize;
    let mut taken = 0usize;

    while taken < char_count {
        let chunk = chunks.next_chunk().await.map_err(AppError::Io)?;
        if chunk.is_empty() {
            break; // EOF
        }

        // 跳过尚未到达的字符偏移
        if skipped < char_offset {
            match char_boundary_at(chunk, char_offset - skipped) {
                // 目标落在本块内：定位到起始边界后开始收集
                Some(byte_off) => {
                    skipped = char_offset;
                    let take: String = chunk[byte_off..].chars().take(char_count - taken).collect();
                    taken += take.chars().count();
                    collected.push_str(&take);
                },
                // 目标在本块之后：整块跳过
                None => skipped += chunk.chars().count(),
            }
        } else {
            let take: String = chunk.chars().take(char_count - taken).collect();
            taken += take.chars().count();
            collected.push_str(&take);
        }
    }

    Ok(collected)
}

/// 第 `n` 个字符的字节偏移；字符数不足 `n + 1` 时返回 `None`
fn char_boundary_at(s: &str, n: usize) -> Option<usize> {
    s.char_indices().nth(n).map(|(i, _)| i)
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

    // 检查书籍存在性与读权限（所有者或公开书）
    check_book_access(&pool, auth.user_id, book_id).await?;

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
    // 注意：position 字段对 txt 是字符偏移；对 epub 是 spine 索引
    Ok(Json(ApiResponse::success(JumpToChapterResponse {
        position: chapter.position,
    })))
}

// 获取公开书籍列表（附带当前请求用户的进度与阅读时长）
async fn list_public_books(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Query(params): Query<PaginationParams>,
) -> Result<Json<ApiResponse<PublicBookListResponse>>, AppError> {
    // 归一化分页参数：page 最小为 1，limit 上限 50
    let (limit, offset) = params.normalize(10, 50);

    // 获取总数（回收站中的书不再出现在公开书库）
    let total = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM books WHERE is_public = 1 AND deleted_at IS NULL",
    )
    .fetch_one(&pool)
    .await?;

    // 获取公开书籍列表
    let books = sqlx::query!(
        r#"
        SELECT b.id, b.title, b.author, b.created_at, b.total_chars, b.total_pages,
               b.description, b.format AS "format!: BookFormat",
               u.username as owner_username,
               rp.percent AS percent, rp.reading_time AS reading_time
        FROM books b
        JOIN users u ON b.user_id = u.id
        LEFT JOIN reading_progress rp ON rp.book_id = b.id AND rp.user_id = ?
        WHERE b.is_public = 1 AND b.deleted_at IS NULL
        ORDER BY b.created_at DESC
        LIMIT ? OFFSET ?
        "#,
        auth.user_id,
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
            format: book.format,
            created_at: book.created_at,
            total_chars: book.total_chars,
            total_pages: book.total_pages,
            description: book.description,
            percent: book.percent,
            reading_time: book.reading_time,
        })
        .collect();

    Ok(Json(ApiResponse::success(PublicBookListResponse {
        total,
        books: book_list,
    })))
}

// 随机获取公开书籍（附带当前请求用户的进度与阅读时长）
async fn get_random_public_books(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Query(params): Query<RandomPublicParams>,
) -> Result<Json<ApiResponse<RandomPublicBooksResponse>>, AppError> {
    // 直接在 SQL 层随机抽样，避免全表加载进内存
    let count = params.count.unwrap_or(1).clamp(1, 10);
    let books = sqlx::query!(
        r#"
        SELECT b.id, b.title, b.author, b.created_at, b.total_chars, b.total_pages,
               b.description, b.format AS "format!: BookFormat",
               u.username as owner_username,
               rp.percent AS percent, rp.reading_time AS reading_time
        FROM books b
        JOIN users u ON b.user_id = u.id
        LEFT JOIN reading_progress rp ON rp.book_id = b.id AND rp.user_id = ?
        WHERE b.is_public = 1 AND b.deleted_at IS NULL AND b.user_id != ?
        ORDER BY RANDOM()
        LIMIT ?
        "#,
        auth.user_id,
        auth.user_id,
        count
    )
    .fetch_all(&pool)
    .await?;

    let selected_books: Vec<PublicBookListItem> = books
        .into_iter()
        .map(|book| PublicBookListItem {
            book_id: book.id,
            title: book.title,
            author: book.author,
            owner_username: book.owner_username,
            format: book.format,
            created_at: book.created_at,
            total_chars: book.total_chars,
            total_pages: book.total_pages,
            description: book.description,
            percent: book.percent,
            reading_time: book.reading_time,
        })
        .collect();

    Ok(Json(ApiResponse::success(RandomPublicBooksResponse {
        books: selected_books,
    })))
}

/// 转义 `SQLite` LIKE 的特殊字符（%, _, \），用 ESCAPE '\' 配合
pub(crate) fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    for ch in input.chars() {
        match ch {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(ch);
            },
            _ => out.push(ch),
        }
    }
    out
}

// 列表/搜索共用的行结构（私用 FromRow，避免在闭包里手动取列）
#[derive(sqlx::FromRow)]
struct BookListRow {
    id: i64,
    title: String,
    author: Option<String>,
    is_public: bool,
    created_at: String,
    total_chars: i64,
    total_pages: i64,
    description: Option<String>,
    format: BookFormat,
    locator: Option<String>,
    percent: Option<f64>,
    reading_time: Option<i64>,
    last_read_at: Option<String>,
}

// 搜索书籍：按 title/author/章节标题 LIKE 模糊匹配
// 不做书内全文索引（FTS）：搜索定位是元数据级别
async fn search_books(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Query(params): Query<SearchParams>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<ApiResponse<BookListResponse>>, AppError> {
    let q = params.q.trim();
    if q.is_empty() {
        return Err(AppError::SearchQueryEmpty);
    }

    let scope = SearchScope::parse(params.scope.as_deref())?;
    // 复用 normalize：钳制 limit 并以 i64 计算 offset，避免 u32 乘法溢出
    let (limit, offset) = pagination.normalize(10, 50);

    // 构造 LIKE 模式：`%query%`，先转义用户输入的特殊字符
    let pattern = format!("%{}%", escape_like(q));

    // 使用 DISTINCT 因为 LEFT JOIN chapters 会让一本书出现多次
    let mut qb = sqlx::QueryBuilder::new(
        "SELECT DISTINCT b.id, b.title, b.author, b.is_public, b.created_at, b.total_chars AS \
         total_chars, b.total_pages AS total_pages, b.description AS description, b.format AS \
         format, rp.locator AS locator, rp.percent AS percent, rp.reading_time AS reading_time, \
         rp.last_read_at AS last_read_at FROM books b LEFT JOIN reading_progress rp ON rp.book_id \
         = b.id AND rp.user_id = ",
    );
    qb.push_bind(auth.user_id);
    qb.push(" LEFT JOIN chapters c ON c.book_id = b.id WHERE (b.title LIKE ");
    qb.push_bind(&pattern);
    qb.push(" ESCAPE '\\' OR b.author LIKE ");
    qb.push_bind(&pattern);
    qb.push(" ESCAPE '\\' OR c.title LIKE ");
    qb.push_bind(&pattern);
    qb.push(" ESCAPE '\\') AND b.deleted_at IS NULL AND (");

    push_scope_condition(&mut qb, scope, auth.user_id);
    qb.push(") ORDER BY b.created_at DESC LIMIT ")
        .push_bind(limit);
    qb.push(" OFFSET ").push_bind(offset);

    let rows = qb.build_query_as::<BookListRow>().fetch_all(&pool).await?;

    let book_list: Vec<BookListItem> = rows
        .into_iter()
        .map(|r| BookListItem {
            book_id: r.id,
            title: r.title,
            author: r.author,
            is_public: r.is_public,
            format: r.format,
            created_at: r.created_at,
            last_read_at: r.last_read_at,
            locator: r.locator,
            percent: r.percent.unwrap_or(0.0),
            reading_time: r.reading_time.unwrap_or(0),
            total_chars: r.total_chars,
            total_pages: r.total_pages,
            description: r.description,
        })
        .collect();

    // 单独查询总数，避免 DISTINCT + JOIN 的复杂聚合影响主查询性能
    let mut count_qb = sqlx::QueryBuilder::new(
        "SELECT COUNT(DISTINCT b.id) FROM books b LEFT JOIN chapters c ON c.book_id = b.id WHERE \
         (b.title LIKE ",
    );
    count_qb.push_bind(&pattern);
    count_qb.push(" ESCAPE '\\' OR b.author LIKE ");
    count_qb.push_bind(&pattern);
    count_qb.push(" ESCAPE '\\' OR c.title LIKE ");
    count_qb.push_bind(&pattern);
    count_qb.push(" ESCAPE '\\') AND b.deleted_at IS NULL AND (");
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
fn push_scope_condition(qb: &mut sqlx::QueryBuilder<Sqlite>, scope: SearchScope, user_id: i64) {
    match scope {
        SearchScope::Mine => {
            qb.push("b.user_id = ").push_bind(user_id);
        },
        SearchScope::Public => {
            qb.push("b.is_public = 1");
        },
        SearchScope::All => {
            qb.push("b.user_id = ")
                .push_bind(user_id)
                .push(" OR b.is_public = 1");
        },
    }
}

// ============ 书内全文搜索（仅 txt） ============

// 书内全文搜索查询参数（区别于书架元数据搜索的 SearchParams）
#[derive(Debug, Deserialize)]
pub struct BookSearchParams {
    pub q: String,
}

// 书内全文搜索：ASCII 大小写不敏感的字面匹配（不分词、不归一化）。
// char_offset 为命中的全书字符偏移，与章节 position、/content 的 position、
// 进度 locator 同一坐标系（chars().count() 语义），可直接用于内容切片定位
async fn search_in_book(
    auth: AuthUser,
    State(pool): State<Pool<Sqlite>>,
    Path(book_id): Path<i64>,
    Query(params): Query<BookSearchParams>,
) -> Result<Json<ApiResponse<BookTextSearchResponse>>, AppError> {
    // 与书架搜索同一语义：trim 后为空 = 无效查询
    let q = params.q.trim();
    if q.is_empty() {
        return Err(AppError::SearchQueryEmpty);
    }
    if q.chars().count() > BOOK_SEARCH_QUERY_MAX_CHARS {
        return Err(AppError::FieldTooLarge("q"));
    }

    let book = sqlx::query!(
        r#"SELECT user_id, file_path, is_public, format AS "format!: BookFormat" FROM books WHERE id = ?"#,
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or(AppError::BookNotFound)?;

    // 读权限与 content / jump_to_chapter 一致：所有者或公开书
    ensure_book_access(auth.user_id, book.user_id, book.is_public)?;

    // 仅 txt；epub/pdf 沿用 content 端点的错误语义
    if book.format != BookFormat::Txt {
        return Err(AppError::ContentEndpointNotSupported);
    }

    // 章节归属表（position 升序；txt 的 position = 章节首字符偏移）
    let chapters: Vec<(String, i64)> =
        sqlx::query_as("SELECT title, position FROM chapters WHERE book_id = ? ORDER BY position")
            .bind(book_id)
            .fetch_all(&pool)
            .await?;

    // 全文扫描为 CPU 密集操作，移入阻塞线程池（参照 parse_upload_sync 的模式）；
    // 多扫一条（cap+1）仅为判断截断，超出部分不返回
    let file_path = book.file_path;
    let query = q.to_string();
    let scanned =
        tokio::task::spawn_blocking(move || -> Result<Vec<BookTextSearchMatch>, AppError> {
            scan_book_file(&file_path, &query, &chapters, BOOK_SEARCH_MAX_MATCHES + 1)
        })
        .await
        .map_err(|e| AppError::Internal(format!("book search task failed: {e}")))??;

    let truncated = scanned.len() > BOOK_SEARCH_MAX_MATCHES;
    let mut matches = scanned;
    matches.truncate(BOOK_SEARCH_MAX_MATCHES);

    Ok(Json(ApiResponse::success(BookTextSearchResponse {
        matches,
        truncated,
    })))
}

/// 单次书内搜索返回的命中上限：超出即停止扫描并以 `truncated` 标注
pub(crate) const BOOK_SEARCH_MAX_MATCHES: usize = 200;

/// 命中前后各取的上下文字符数（excerpt 长度 ≤ 2×该值 + 命中词长）
const BOOK_SEARCH_EXCERPT_CONTEXT: usize = 30;

/// 读入书籍文件并执行全文扫描。整文件读入的峰值内存受 max_file_size 约束，
/// 与上传解析（[`parse_upload_sync`]）同一策略
fn scan_book_file(
    path: &str,
    query: &str,
    chapters: &[(String, i64)],
    cap: usize,
) -> Result<Vec<BookTextSearchMatch>, AppError> {
    let bytes = std::fs::read(path).map_err(AppError::Io)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        // 上传时已保证 UTF-8：失败即文件被外部篡改，沿用 Io 错误语义（500）
        AppError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "book file is not valid UTF-8",
        ))
    })?;
    Ok(scan_matches(text, query, chapters, cap))
}

/// 在 `text` 中找出 `query` 的全部命中并换算为字符偏移、附上章节归属与摘录。
/// 字节级 ASCII 大小写不敏感比较：UTF-8 自同步编码保证命中只会落在字符边界，
/// 非 ASCII 字符不受 case fold 影响（等价于原样匹配）。命中数达到 `cap` 即停止；
/// 重叠命中（如 "aa" 匹配 "aaa"）逐位置各计一条
fn scan_matches(
    text: &str,
    query: &str,
    chapters: &[(String, i64)],
    cap: usize,
) -> Vec<BookTextSearchMatch> {
    let needle: Vec<u8> = query.bytes().map(|b| b.to_ascii_lowercase()).collect();
    let mut matches = Vec::new();
    // 已换算字节前缀 → 字符数的单遍游标：避免每条命中都重扫文件前缀
    let (mut done_byte, mut done_char) = (0usize, 0usize);

    let mut from = 0usize;
    while matches.len() < cap
        && let Some(hit) = find_fold(text.as_bytes(), &needle, from)
    {
        done_char += text[done_byte..hit].chars().count();
        done_byte = hit;

        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let (chapter_index, chapter_title, char_offset) = {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let offset = done_char as i64;
            let (index, title) = chapter_at(chapters, offset);
            (index, title, offset)
        };
        matches.push(BookTextSearchMatch {
            chapter_index,
            chapter_title,
            char_offset,
            excerpt: excerpt_around(text, hit, needle.len(), BOOK_SEARCH_EXCERPT_CONTEXT),
        });
        from = hit + 1;
    }
    matches
}

/// 从 `from` 字节起查找 `needle` 的首次 ASCII 大小写不敏感匹配，
/// 返回命中起始字节偏移；`needle` 非空由调用方保证
fn find_fold(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let first = needle[0];
    let last_start = haystack.len().checked_sub(needle.len())?;
    (from..=last_start).find(|&i| {
        haystack[i].to_ascii_lowercase() == first
            && haystack[i + 1..i + needle.len()]
                .iter()
                .zip(&needle[1..])
                .all(|(&h, &n)| h.to_ascii_lowercase() == n)
    })
}

/// 二分定位命中所属章节，返回章节列表下标（0-based）与标题。
/// txt 首章起于首个非空行，正常命中必落在某章内；空章节表或越前（防御）回退 (0, "")
fn chapter_at(chapters: &[(String, i64)], char_offset: i64) -> (i64, String) {
    let hit = chapters
        .partition_point(|(_, position)| *position <= char_offset)
        .checked_sub(1);
    match hit.and_then(|i| chapters.get(i)) {
        Some((title, _)) => {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let index = hit.unwrap_or_default() as i64;
            (index, title.clone())
        },
        None => (0, String::new()),
    }
}

/// 取命中前后各 `ctx` 个字符的纯文本上下文（含命中本身；文本不足时截到边界）
fn excerpt_around(text: &str, hit: usize, match_bytes: usize, ctx: usize) -> String {
    // 前文不足 ctx 字符时从头取（nth 越界返回 None → 0）
    let start = text[..hit]
        .char_indices()
        .rev()
        .nth(ctx - 1)
        .map_or(0, |(i, _)| i);
    let end = hit
        + match_bytes
        + text[hit + match_bytes..]
            .chars()
            .take(ctx)
            .map(char::len_utf8)
            .sum::<usize>();
    text[start..end].to_string()
}

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
async fn get_book_tags(
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
async fn set_book_tags(
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

#[cfg(test)]
mod tests {
    use super::{
        BOOK_SEARCH_MAX_MATCHES, build_content_disposition, chapter_at, excerpt_around, find_fold,
        parse_upload_sync, read_char_slice, scan_matches,
    };
    use crate::{error::AppError, models::BookFormat, utils::utf8::UTF8_CHUNK_SIZE};

    #[test]
    fn parse_upload_sync_txt_metadata_and_hash() {
        let content = "第一章 开始\n内容\n第二章 结尾\n";
        let parsed = parse_upload_sync(
            content.as_bytes(),
            String::new(),
            None,
            None,
            None,
            Some("我的书.txt"),
        )
        .unwrap();
        assert_eq!(parsed.format, BookFormat::Txt);
        assert_eq!(parsed.chapters.len(), 2);
        // 文件名兜底：去扩展名
        assert_eq!(parsed.title, "我的书");
        assert_eq!(
            parsed.total_chars,
            i64::try_from(content.chars().count()).unwrap()
        );
        assert_eq!(
            parsed.content_hash,
            crate::utils::sha256_hex(content.as_bytes())
        );
    }

    #[test]
    fn parse_upload_sync_description_normalized() {
        // trim + 纯空白视为未提供
        let parsed = parse_upload_sync(
            "内容\n".as_bytes(),
            "书名".into(),
            None,
            Some("  简介  ".into()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(parsed.description.as_deref(), Some("简介"));
        let parsed = parse_upload_sync(
            "内容\n".as_bytes(),
            "书名".into(),
            None,
            Some("   ".into()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(parsed.description, None);

        // 超限拒绝（400 语义）
        let over = "界".repeat(crate::limits::BOOK_DESCRIPTION_MAX_CHARS + 1);
        assert!(matches!(
            parse_upload_sync(
                "内容\n".as_bytes(),
                "书名".into(),
                None,
                Some(over),
                None,
                None
            )
            .err(),
            Some(AppError::FieldTooLarge("description"))
        ));
    }

    #[test]
    fn parse_upload_sync_title_empty_rejected() {
        // 用户输入与文件名兜底均为空：TitleEmpty
        assert!(matches!(
            parse_upload_sync("内容\n".as_bytes(), "  ".into(), None, None, None, None).err(),
            Some(AppError::TitleEmpty)
        ));
    }

    #[test]
    fn parse_upload_sync_pattern_checked_before_parse() {
        // 非 txt 携带非空 chapter_pattern：在解析前即拒绝
        assert!(matches!(
            parse_upload_sync(b"%PDF-1.4\n", "t".into(), None, None, Some("^第.章"), None).err(),
            Some(AppError::ChapterPatternNotSupported)
        ));
    }

    #[test]
    fn content_disposition_ascii_fallback_keeps_printable() {
        let d = build_content_disposition(true, "My Book", Some("Jane Doe"), 7, BookFormat::Epub);
        assert_eq!(
            d,
            "attachment; filename=\"My Book - Jane Doe.epub\"; \
             filename*=UTF-8''My%20Book%20-%20Jane%20Doe.epub"
        );
        // 空白作者视作缺省：仅书名
        let d = build_content_disposition(true, "My Book", Some("  "), 7, BookFormat::Epub);
        assert_eq!(
            d,
            "attachment; filename=\"My Book.epub\"; filename*=UTF-8''My%20Book.epub"
        );
    }

    #[tokio::test]
    async fn read_char_slice_handles_multibyte_boundary() {
        // 构造跨块边界的多字节字符
        let mut content = String::new();
        for _ in 0..UTF8_CHUNK_SIZE - 1 {
            content.push('a');
        }
        content.push_str("世界🌍bb");
        let tmp = std::env::temp_dir().join("reader_test_read_char_slice.txt");
        tokio::fs::write(&tmp, &content).await.unwrap();

        let path = tmp.to_str().unwrap();

        // 1) 起点落在横跨块边界的字符上，取 4 个字符 → "世界🌍b"
        let s = read_char_slice(path, UTF8_CHUNK_SIZE - 1, 4).await.unwrap();
        assert_eq!(s, "世界🌍b", "char straddling chunk boundary");

        // 2) 从 emoji（CHUNK+1）取 3 个字符 → "🌍bb"
        let s = read_char_slice(path, UTF8_CHUNK_SIZE + 1, 3).await.unwrap();
        assert_eq!(s, "🌍bb");

        // 3) 跨边界的混合起点 → "a世界"
        let s = read_char_slice(path, UTF8_CHUNK_SIZE - 2, 3).await.unwrap();
        assert_eq!(s, "a世界");

        // 4) 全文长度
        let s = read_char_slice(path, 0, 100_000).await.unwrap();
        assert_eq!(s.chars().count(), content.chars().count());

        let _ = tokio::fs::remove_file(&tmp).await;
    }

    #[test]
    fn scan_matches_case_insensitive_and_char_offsets() {
        // ASCII 大小写不敏感；多字节字符按 chars().count() 计入偏移
        let text = "第一章 A\nHello WORLD，世界 hello\n第二章 B\nend HELLO";
        let chapters = vec![
            ("第一章 A".to_string(), 0),
            // "第一章 A\n"(6) + "Hello WORLD，世界 hello\n"(21) = 27
            ("第二章 B".to_string(), 27),
        ];
        let matches = scan_matches(text, "hello", &chapters, BOOK_SEARCH_MAX_MATCHES + 1);
        assert_eq!(matches.len(), 3);
        // 偏移必须指向原文中的命中位置（chars().count() 语义）
        let chars: Vec<char> = text.chars().collect();
        for m in &matches {
            let offset = usize::try_from(m.char_offset).unwrap();
            let hit: String = chars[offset..offset + 5].iter().collect();
            assert_eq!(hit.to_ascii_lowercase(), "hello");
        }
        assert_eq!(matches[0].char_offset, 6);
        assert_eq!(matches[0].chapter_index, 0);
        assert_eq!(matches[0].chapter_title, "第一章 A");
        assert_eq!(matches[2].chapter_index, 1);
        assert_eq!(matches[2].chapter_title, "第二章 B");
        // excerpt 含命中本身与前后文
        assert!(matches[0].excerpt.contains("Hello WORLD"));
    }

    #[test]
    fn scan_matches_no_match_and_overlap() {
        assert!(scan_matches("abc", "xyz", &[], 201).is_empty());
        // 重叠命中逐位置各计一条
        let hits = scan_matches("aaa", "aa", &[], 201);
        assert_eq!(hits.iter().map(|m| m.char_offset).collect::<Vec<_>>(), [
            0, 1
        ]);
    }

    #[test]
    fn scan_matches_truncates_at_cap() {
        let text = "x".repeat(250);
        // cap=201：扫描到 201 条即停止，调用方据此判断截断
        let hits = scan_matches(&text, "x", &[], BOOK_SEARCH_MAX_MATCHES + 1);
        assert_eq!(hits.len(), BOOK_SEARCH_MAX_MATCHES + 1);
    }

    #[test]
    fn excerpt_around_context_boundaries() {
        let text = "0123456789".repeat(10); // 100 ASCII 字符
        // 命中在开头：前文不足 30 字符时从头取
        assert_eq!(excerpt_around(&text, 0, 2, 3), "01234");
        // 命中在中部：前后各 3 字符（"789" + "01" + "234"）
        assert_eq!(excerpt_around(&text, 50, 2, 3), "78901234");
        // 命中在结尾：后文不足时截到边界（"567" + "89"）
        assert_eq!(excerpt_around(&text, 98, 2, 3), "56789");
        // 多字节字符按字符（非字节）计数：前文仅 2 字符全取，后文取 2 字符
        let cjk = "一二三四五六七八九十";
        assert_eq!(excerpt_around(cjk, 6, 3, 2), "一二三四五");
    }

    #[test]
    fn chapter_at_locates_by_position() {
        let chapters = vec![
            ("a".to_string(), 0),
            ("b".to_string(), 10),
            ("c".to_string(), 20),
        ];
        assert_eq!(chapter_at(&chapters, 0), (0, "a".to_string()));
        assert_eq!(chapter_at(&chapters, 9), (0, "a".to_string()));
        assert_eq!(chapter_at(&chapters, 10), (1, "b".to_string()));
        assert_eq!(chapter_at(&chapters, 999), (2, "c".to_string()));
        // 空章节表 / 越前：防御回退
        assert_eq!(chapter_at(&[], 5), (0, String::new()));
    }

    #[test]
    fn find_fold_ascii_case_fold_only() {
        assert_eq!(find_fold(b"Hello World", b"world", 0), Some(6));
        assert_eq!(find_fold(b"abcabc", b"cab", 0), Some(2));
        // from 之后无命中
        assert_eq!(find_fold(b"abcabc", b"abc", 3), Some(3));
        assert_eq!(find_fold(b"abcabc", b"abc", 4), None);
        // needle 比正文长
        assert_eq!(find_fold(b"ab", b"abc", 0), None);
        // 非 ASCII 字节不受 fold 影响（原样匹配）
        assert_eq!(find_fold("世界".as_bytes(), "世界".as_bytes(), 0), Some(0));
        assert_eq!(find_fold("世界".as_bytes(), "界世".as_bytes(), 0), None);
    }
}
