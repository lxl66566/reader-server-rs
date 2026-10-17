use std::{
    num::NonZeroUsize,
    sync::{Arc, LazyLock, Mutex, PoisonError},
};

use axum::{
    Json,
    extract::{Path, Query, State},
};
use lru::LruCache;
use serde::Deserialize;
use sqlx::{Pool, Sqlite};

use super::helpers::{BookListRow, PaginationParams, escape_like};
use crate::{
    access::ensure_book_access,
    auth::AuthUser,
    error::{ApiResponse, AppError},
    limits::BOOK_SEARCH_QUERY_MAX_CHARS,
    models::{
        BookFormat, BookListItem, BookListResponse, BookTextSearchMatch, BookTextSearchResponse,
    },
    parser::epub::parse_epub,
};

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

// 搜索书籍：按 title/author/章节标题 LIKE 模糊匹配
// 不做书内全文索引（FTS）：搜索定位是元数据级别
pub(super) async fn search_books(
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

// ============ 书内全文搜索（txt / epub） ============

// 书内全文搜索查询参数（区别于书架元数据搜索的 SearchParams）
#[derive(Debug, Deserialize)]
pub struct BookSearchParams {
    pub q: String,
}

// 书内全文搜索：ASCII 大小写不敏感的字面匹配（不分词、不归一化）。
// char_offset 为命中的全书字符偏移，与章节 position、/content 的 position、
// 进度 locator 同一坐标系（txt）或与解析全文对齐（epub），可直接定位
pub(super) async fn search_in_book(
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
        r#"SELECT user_id, file_path, content_hash, is_public, format AS "format!: BookFormat" FROM books WHERE id = ?"#,
        book_id
    )
    .fetch_optional(&pool)
    .await?
    .ok_or(AppError::BookNotFound)?;

    // 读权限与 content / jump_to_chapter 一致：所有者或公开书
    ensure_book_access(auth.user_id, book.user_id, book.is_public)?;

    // 章节归属表（position 升序；txt 的 position = 章节首字符偏移）。
    // epub 的章节定位走解析产物的 char_offset（与 DB 中 spine 顺序一致），无需读库
    let chapters = if book.format == BookFormat::Txt {
        sqlx::query_as("SELECT title, position FROM chapters WHERE book_id = ? ORDER BY position")
            .bind(book_id)
            .fetch_all(&pool)
            .await?
    } else {
        Vec::new()
    };

    // 全文扫描为 CPU 密集操作，移入阻塞线程池（参照 parse_upload_sync 的模式）；
    // 多扫一条（cap+1）仅为判断截断，超出部分不返回
    let file_path = book.file_path;
    let format = book.format;
    let content_hash = book.content_hash;
    let query = q.to_string();
    let scanned =
        tokio::task::spawn_blocking(move || -> Result<Vec<BookTextSearchMatch>, AppError> {
            let cap = BOOK_SEARCH_MAX_MATCHES + 1;
            match format {
                BookFormat::Txt => scan_book_file(&file_path, &query, &chapters, cap),
                BookFormat::Epub => scan_epub_file(&file_path, &query, &content_hash, cap),
                // pdf 无服务端文本层，沿用 content 端点的错误语义
                BookFormat::Pdf => Err(AppError::ContentEndpointNotSupported),
            }
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

/// epub 解析产物中搜索所需的部分（解析一次，多次查询共享）
struct EpubSearchText {
    text: String,
    /// (章节标题, 章节首字符在 text 中的偏移)，顺序 = spine 顺序，与 DB chapters 一致
    chapters: Vec<(String, i64)>,
}

/// epub 全文解析缓存：按 content_hash 键控（书文件不可变，重传生成新 hash，
/// 也天然隔离测试间同 id 的不同书）。容量 2 覆盖当前热门书；检查/插入持锁、
/// 解析在锁外，避免慢解析阻塞其他书的并发查询
static EPUB_TEXT_CACHE: LazyLock<Mutex<LruCache<String, Arc<EpubSearchText>>>> =
    LazyLock::new(|| {
        Mutex::new(LruCache::new(
            NonZeroUsize::new(2).expect("non-zero cache capacity"),
        ))
    });

/// 读入 epub 文件并解析出可搜索文本（带缓存；并发未命中会重复解析，可接受）
fn cached_epub_text(path: &str, content_hash: &str) -> Result<Arc<EpubSearchText>, AppError> {
    let mut cache = EPUB_TEXT_CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(hit) = cache.get(content_hash) {
        return Ok(Arc::clone(hit));
    }
    drop(cache);

    let bytes = std::fs::read(path).map_err(AppError::Io)?;
    let parsed = parse_epub(&bytes)?;
    let entry = Arc::new(EpubSearchText {
        text: parsed.text_content,
        chapters: parsed
            .chapters
            .iter()
            .map(|c| (c.title.clone(), c.char_offset))
            .collect(),
    });
    EPUB_TEXT_CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .put(content_hash.to_string(), Arc::clone(&entry));
    Ok(entry)
}

/// epub 搜索：拼接全文（段落纯文本，仅 `<p>`，与 total_chars 同源）上做字面匹配
fn scan_epub_file(
    path: &str,
    query: &str,
    content_hash: &str,
    cap: usize,
) -> Result<Vec<BookTextSearchMatch>, AppError> {
    let text = cached_epub_text(path, content_hash)?;
    Ok(scan_matches(&text.text, query, &text.chapters, cap))
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

#[cfg(test)]
mod tests {
    use super::{BOOK_SEARCH_MAX_MATCHES, chapter_at, excerpt_around, find_fold, scan_matches};

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
