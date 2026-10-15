use axum::{
    Json,
    extract::{Query, State},
};
use serde::Deserialize;
use sqlx::{Pool, Sqlite};

use super::helpers::{BookListRow, PaginationParams};
use crate::{
    auth::AuthUser,
    error::{ApiResponse, AppError},
    models::{
        BookFormat, BookListItem, BookListResponse, PublicBookListItem, PublicBookListResponse,
        RandomPublicBooksResponse,
    },
};

// 随机公开书籍查询参数
#[derive(Debug, Deserialize)]
pub struct RandomPublicParams {
    pub count: Option<i64>,
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

// 获取用户书籍列表
pub(super) async fn list_books(
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

// 获取公开书籍列表（附带当前请求用户的进度与阅读时长）
pub(super) async fn list_public_books(
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
pub(super) async fn get_random_public_books(
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
