//! 用户数据的导出 / 导入（迁移）：zip 包格式定义、构建与恢复。
//!
//! 包结构：
//! - `metadata.json`：[`UserExport`] 的 JSON 序列化
//! - `{idx:03}_{title}.{ext}`：书籍原文件，`idx` 为 `metadata.books` 数组下标
//!
//! 导入兼容历史用户自助导出的无版本号包：`books[].id` 引用、`reading_settings`
//! 数组形态、条目名按 `{idx:03}_` 前缀回退匹配。

use std::{
    collections::HashMap,
    io::Read as _,
    path::{Path, PathBuf},
};

use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Pool, Sqlite};

use crate::{
    error::AppError,
    models::{
        self, AnnotationKind, BookFormat, ChineseConversion, PaginationMode, ReadingTheme,
        UserStatus,
    },
    utils::{generate_uuid, sha256_hex},
};

/// 当前导出格式版本；导入拒绝更大版本（来自更新服务器的包）
pub(crate) const EXPORT_FORMAT_VERSION: u32 = 2;

/// metadata.json 条目的大小上限：zip 条目头的 size 可伪造，不能信任（防炸弹）
const METADATA_ENTRY_MAX: usize = 128 * 1024 * 1024;

/// 导出范围：决定包内是否携带凭据字段
#[derive(Debug, Clone, Copy)]
pub(crate) enum ExportScope {
    /// 用户自助导出：不含密码哈希（导出包可能被用户转发给第三方）
    SelfService,
    /// 管理员迁移导出：含密码哈希，导入后用户可凭原密码登录
    Migration,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct UserExport {
    /// 导出格式版本；历史自助导出无此字段，按 0 处理
    #[serde(default)]
    pub version: u32,
    pub user: ExportUser,
    /// 密码哈希：仅管理员迁移导出包含，导入时作为缺省凭据
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_hash: Option<String>,
    pub books: Vec<ExportBook>,
    pub chapters: Vec<ExportChapter>,
    pub reading_progress: Vec<ExportProgress>,
    /// 0/1 个元素（reading_settings.user_id UNIQUE）；数组形态是对历史格式的兼容
    pub reading_settings: Vec<ExportSettings>,
    pub bookmarks: Vec<ExportBookmark>,
    pub annotations: Vec<ExportAnnotation>,
    #[serde(default)]
    pub daily_reading_stats: Vec<ExportDailyStat>,
    pub exported_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ExportUser {
    pub username: String,
    pub created_at: String,
    pub total_reading_time: i64,
    #[serde(default)]
    pub status: UserStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ExportBook {
    /// 包内书籍标识（章节 / 进度 / 书签 / 批注引用它）；导入时重映射为新 id。
    /// 历史格式来自 books.id，用 alias 兼容
    #[serde(alias = "id")]
    pub book_id: i64,
    pub title: String,
    pub author: Option<String>,
    pub is_public: bool,
    pub format: BookFormat,
    pub total_chars: i64,
    pub total_pages: i64,
    pub description: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 书籍文件在 zip 内的条目名；历史格式无此字段，按下标前缀回退匹配
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromRow)]
pub(crate) struct ExportChapter {
    pub book_id: i64,
    pub title: String,
    pub position: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromRow)]
pub(crate) struct ExportProgress {
    pub book_id: i64,
    pub locator: String,
    pub percent: f64,
    pub reading_time: i64,
    pub last_read_at: Option<String>,
    #[serde(default)]
    pub last_device_id: Option<String>,
}

/// 阅读设置（不含 id/user_id；导入时挂到新用户）
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ExportSettings {
    pub font_size: i64,
    pub background_color: String,
    pub text_color: String,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub paragraph_spacing: f64,
    pub reading_width: i64,
    pub text_indent: f64,
    pub chinese_conversion: ChineseConversion,
    pub timezone: String,
    pub font_family: String,
    pub theme: ReadingTheme,
    pub pagination_mode: PaginationMode,
}

impl From<models::ReadingSettings> for ExportSettings {
    fn from(s: models::ReadingSettings) -> Self {
        ExportSettings {
            font_size: s.font_size,
            background_color: s.background_color,
            text_color: s.text_color,
            line_height: s.line_height,
            letter_spacing: s.letter_spacing,
            paragraph_spacing: s.paragraph_spacing,
            reading_width: s.reading_width,
            text_indent: s.text_indent,
            chinese_conversion: s.chinese_conversion,
            timezone: s.timezone,
            font_family: s.font_family,
            theme: s.theme,
            pagination_mode: s.pagination_mode,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, FromRow)]
pub(crate) struct ExportBookmark {
    pub book_id: i64,
    pub locator: String,
    pub percent: f64,
    pub label: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromRow)]
pub(crate) struct ExportAnnotation {
    pub book_id: i64,
    pub kind: AnnotationKind,
    pub start_locator: String,
    pub end_locator: Option<String>,
    pub start_percent: f64,
    pub end_percent: Option<f64>,
    pub excerpt: Option<String>,
    pub content: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, FromRow)]
pub(crate) struct ExportDailyStat {
    pub date: String,
    pub reading_seconds: i64,
}

pub(crate) struct UserExportBundle {
    pub export: UserExport,
    /// (zip 条目名, 书籍原文件路径)
    pub files: Vec<(String, PathBuf)>,
}

/// 从数据库收集用户全部数据。书籍文件路径按 `metadata.books` 下标命名条目
pub(crate) async fn collect_user_export(
    pool: &Pool<Sqlite>,
    user_id: i64,
    scope: ExportScope,
) -> Result<UserExportBundle, AppError> {
    let user = sqlx::query_as::<_, models::User>("SELECT * FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::UserNotFound)?;

    let book_rows: Vec<models::Book> =
        sqlx::query_as::<_, models::Book>("SELECT * FROM books WHERE user_id = ? ORDER BY id")
            .bind(user_id)
            .fetch_all(pool)
            .await?;

    // 标签按书分组
    let tag_rows = sqlx::query!(
        "SELECT b.id AS book_id, t.name FROM books b JOIN book_tags bt ON bt.book_id = b.id JOIN \
         tags t ON t.id = bt.tag_id WHERE b.user_id = ? ORDER BY b.id, t.name",
        user_id
    )
    .fetch_all(pool)
    .await?;
    let mut tags_map: HashMap<i64, Vec<String>> = HashMap::new();
    for row in tag_rows {
        tags_map.entry(row.book_id).or_default().push(row.name);
    }

    let book_ids: Vec<i64> = book_rows.iter().map(|b| b.id).collect();
    let chapters: Vec<ExportChapter> = if book_ids.is_empty() {
        Vec::new()
    } else {
        let mut qb = sqlx::QueryBuilder::new(
            "SELECT book_id, title, position FROM chapters WHERE book_id IN (",
        );
        let mut sep = qb.separated(", ");
        for id in &book_ids {
            sep.push_bind(id);
        }
        qb.push(")");
        qb.build_query_as::<ExportChapter>().fetch_all(pool).await?
    };

    let settings: Vec<ExportSettings> = sqlx::query_as::<_, models::ReadingSettings>(
        "SELECT * FROM reading_settings WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(Into::into)
    .collect();

    let bookmarks: Vec<ExportBookmark> = sqlx::query_as::<_, ExportBookmark>(
        "SELECT book_id, locator, percent, label, created_at FROM bookmarks WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let annotations: Vec<ExportAnnotation> = sqlx::query_as::<_, ExportAnnotation>(
        "SELECT book_id, kind, start_locator, end_locator, start_percent, end_percent, excerpt, \
         content, created_at, updated_at FROM annotations WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let progress: Vec<ExportProgress> = sqlx::query_as::<_, ExportProgress>(
        "SELECT book_id, locator, percent, reading_time, last_read_at, last_device_id FROM \
         reading_progress WHERE user_id = ?",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let daily: Vec<ExportDailyStat> = sqlx::query_as::<_, ExportDailyStat>(
        "SELECT date, reading_seconds FROM daily_reading_stats WHERE user_id = ? ORDER BY date",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    // 条目名与 books 数组下标一一对应，导入按下标前缀或显式 file 字段定位
    let mut files = Vec::with_capacity(book_rows.len());
    let books: Vec<ExportBook> = book_rows
        .into_iter()
        .enumerate()
        .map(|(idx, b)| {
            let entry = format!(
                "{idx:03}_{}.{}",
                sanitize_filename(&b.title),
                b.format.ext()
            );
            files.push((entry.clone(), PathBuf::from(&b.file_path)));
            ExportBook {
                book_id: b.id,
                title: b.title,
                author: b.author,
                is_public: b.is_public,
                format: b.format,
                total_chars: b.total_chars,
                total_pages: b.total_pages,
                description: b.description,
                created_at: b.created_at,
                tags: tags_map.remove(&b.id).unwrap_or_default(),
                file: Some(entry),
            }
        })
        .collect();

    Ok(UserExportBundle {
        export: UserExport {
            version: EXPORT_FORMAT_VERSION,
            user: ExportUser {
                username: user.username,
                created_at: user.created_at,
                total_reading_time: user.total_reading_time,
                status: user.status,
            },
            password_hash: match scope {
                ExportScope::SelfService => None,
                ExportScope::Migration => Some(user.password_hash),
            },
            books,
            chapters,
            reading_progress: progress,
            reading_settings: settings,
            bookmarks,
            annotations,
            daily_reading_stats: daily,
            exported_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        },
        files,
    })
}

/// 构建用户数据导出 zip（写入临时文件），返回 (zip 路径, 下载文件名)。
/// 书籍文件用 io::copy 搬运，内存占用与书数量无关；任何文件缺失/读取失败
/// 都整体报错，绝不产出残缺包（迁移场景完整性优先）
pub(crate) async fn build_user_export_zip(
    pool: &Pool<Sqlite>,
    user_id: i64,
    scope: ExportScope,
) -> Result<(PathBuf, String), AppError> {
    let bundle = collect_user_export(pool, user_id, scope).await?;
    let username = bundle.export.user.username.clone();
    let metadata_bytes = serde_json::to_vec_pretty(&bundle.export)
        .map_err(|e| AppError::Internal(format!("failed to serialize metadata: {e}")))?;
    let files = bundle.files;

    let (zip_path, _) = crate::utils::zip::write_temp_zip(move |zip| {
        use std::io::Write as _;

        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let zip_err = crate::utils::zip::zip_write_err;

        zip.start_file("metadata.json", opts).map_err(zip_err)?;
        zip.write_all(&metadata_bytes).map_err(AppError::Io)?;

        for (entry, file_path) in &files {
            let mut file = std::fs::File::open(file_path).map_err(AppError::Io)?;
            zip.start_file(entry.as_str(), opts).map_err(zip_err)?;
            std::io::copy(&mut file, zip).map_err(AppError::Io)?;
        }
        Ok(())
    })
    .await?;

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    Ok((zip_path, format!("{username}_{timestamp}.zip")))
}

/// 从导出包恢复用户。`zip_path` 指向已落盘的 zip（multipart 流式接收的临时文件），
/// 本函数不负责删除它。`username_override` / `password` 为空串视为未提供
pub(crate) async fn restore_user_export(
    pool: &Pool<Sqlite>,
    config: &crate::config::Config,
    zip_path: &Path,
    username_override: Option<String>,
    password: Option<String>,
) -> Result<models::ImportUserResponse, AppError> {
    // 阶段一（阻塞）：解析 metadata 并校验，不写任何文件
    let parse_zip_path = zip_path.to_path_buf();
    let export = tokio::task::spawn_blocking(move || -> Result<UserExport, AppError> {
        let file = std::fs::File::open(&parse_zip_path).map_err(AppError::Io)?;
        let mut zip = zip::ZipArchive::new(file).map_err(crate::utils::zip::zip_write_err)?;
        let metadata = read_entry_limited(
            &mut zip
                .by_name("metadata.json")
                .map_err(|_| AppError::ImportPackageInvalid("metadata.json not found".into()))?,
            METADATA_ENTRY_MAX,
        )?;
        let export: UserExport = serde_json::from_slice(&metadata).map_err(|e| {
            AppError::ImportPackageInvalid(format!("failed to parse metadata.json: {e}"))
        })?;
        validate_export(&export)?;
        Ok(export)
    })
    .await
    .map_err(|e| AppError::Internal(format!("import parse task failed: {e}")))??;

    // 阶段二：凭据决策（显式输入优先，缺省回退到包内值）。
    // 放在文件提取之前：校验失败直接返回，不留已写文件
    let username = match non_empty(username_override) {
        Some(u) => u,
        None => export.user.username.clone(),
    };
    crate::routes::auth::validate_username(&username)?;
    let password_hash = match non_empty(password) {
        Some(p) => {
            crate::routes::auth::validate_password(&p)?;
            crate::utils::hash_password(&p)?
        },
        // 自助导出包不含哈希，必须显式提供密码
        None => export
            .password_hash
            .clone()
            .ok_or(AppError::PasswordEmpty)?,
    };

    // 阶段三（阻塞）：提取书籍文件到 book_dir
    let book_dir = config.storage.book_dir.clone();
    let max_file_size = config.storage.max_file_size;
    let extract_zip_path = zip_path.to_path_buf();
    // clone 一份交给 'static 闭包，体积远小于书籍文件本体
    let export_for_extract = export.clone();
    let files = tokio::task::spawn_blocking(move || -> Result<Vec<ExtractedBook>, AppError> {
        let file = std::fs::File::open(&extract_zip_path).map_err(AppError::Io)?;
        let mut zip = zip::ZipArchive::new(file).map_err(crate::utils::zip::zip_write_err)?;
        extract_books(&mut zip, &export_for_extract, &book_dir, max_file_size)
    })
    .await
    .map_err(|e| AppError::Internal(format!("import extract task failed: {e}")))??;

    // 章节与进度按书预分组，避免逐书全量扫描
    let mut chapters_by_book: HashMap<i64, Vec<&ExportChapter>> = HashMap::new();
    for c in &export.chapters {
        chapters_by_book.entry(c.book_id).or_default().push(c);
    }
    let progress_by_book: HashMap<i64, &ExportProgress> = export
        .reading_progress
        .iter()
        .map(|p| (p.book_id, p))
        .collect();

    // 全部 DB 写入单事务；失败回滚并清理已写书籍文件（不留孤儿文件）
    let db_result: Result<i64, AppError> = async {
        let mut tx = pool.begin().await?;

        let new_user_id = match sqlx::query!(
            "INSERT INTO users (username, password_hash, created_at, total_reading_time, status) \
             VALUES (?, ?, ?, ?, ?)",
            username,
            password_hash,
            export.user.created_at,
            export.user.total_reading_time,
            export.user.status,
        )
        .execute(&mut *tx)
        .await
        {
            Ok(r) => r.last_insert_rowid(),
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                return Err(AppError::UsernameAlreadyExists);
            },
            Err(e) => return Err(e.into()),
        };

        // 阅读设置：导出缺失时按注册默认创建
        match export.reading_settings.first() {
            Some(s) => {
                sqlx::query!(
                    "INSERT INTO reading_settings (user_id, font_size, background_color, \
                     text_color, line_height, letter_spacing, paragraph_spacing, reading_width, \
                     text_indent, chinese_conversion, timezone, font_family, theme, \
                     pagination_mode) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    new_user_id,
                    s.font_size,
                    s.background_color,
                    s.text_color,
                    s.line_height,
                    s.letter_spacing,
                    s.paragraph_spacing,
                    s.reading_width,
                    s.text_indent,
                    s.chinese_conversion,
                    s.timezone,
                    s.font_family,
                    s.theme,
                    s.pagination_mode,
                )
                .execute(&mut *tx)
                .await?;
            },
            None => {
                sqlx::query!(
                    "INSERT INTO reading_settings (user_id) VALUES (?)",
                    new_user_id
                )
                .execute(&mut *tx)
                .await?;
            },
        }

        let mut id_map: HashMap<i64, i64> = HashMap::with_capacity(export.books.len());
        for (book, file) in export.books.iter().zip(files.iter()) {
            let new_book_id = sqlx::query!(
                "INSERT INTO books (user_id, title, author, file_path, is_public, total_chars, \
                 total_pages, description, format, content_hash, created_at) VALUES (?, ?, ?, ?, \
                 ?, ?, ?, ?, ?, ?, ?)",
                new_user_id,
                book.title,
                book.author,
                file.path.to_string_lossy(),
                book.is_public,
                book.total_chars,
                book.total_pages,
                book.description,
                book.format,
                file.content_hash,
                book.created_at,
            )
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
            id_map.insert(book.book_id, new_book_id);

            // 章节批量插入（单次 roundtrip）
            if let Some(chapters) = chapters_by_book.get(&book.book_id) {
                let mut qb =
                    sqlx::QueryBuilder::new("INSERT INTO chapters (book_id, title, position) ");
                qb.push_values(chapters.iter(), |mut b, c| {
                    b.push_bind(new_book_id)
                        .push_bind(c.title.clone())
                        .push_bind(c.position);
                });
                qb.build().execute(&mut *tx).await?;
            }

            // 标签（与 set_book_tags 同一净化规则）
            for tag in crate::routes::books::clean_tags(book.tags.iter().cloned()) {
                sqlx::query!("INSERT OR IGNORE INTO tags (name) VALUES (?)", tag)
                    .execute(&mut *tx)
                    .await?;
                let tag_id: i64 = sqlx::query_scalar!("SELECT id FROM tags WHERE name = ?", tag)
                    .fetch_one(&mut *tx)
                    .await?;
                sqlx::query!(
                    "INSERT OR IGNORE INTO book_tags (book_id, tag_id) VALUES (?, ?)",
                    new_book_id,
                    tag_id
                )
                .execute(&mut *tx)
                .await?;
            }

            // 进度：导出无该书记录时创建初始行（与上传行为一致）
            match progress_by_book.get(&book.book_id) {
                Some(p) => {
                    sqlx::query!(
                        "INSERT INTO reading_progress (user_id, book_id, locator, percent, \
                         reading_time, last_read_at, last_device_id) VALUES (?, ?, ?, ?, ?, ?, ?)",
                        new_user_id,
                        new_book_id,
                        p.locator,
                        p.percent,
                        p.reading_time,
                        p.last_read_at,
                        p.last_device_id,
                    )
                    .execute(&mut *tx)
                    .await?;
                },
                None => {
                    sqlx::query!(
                        "INSERT INTO reading_progress (user_id, book_id) VALUES (?, ?)",
                        new_user_id,
                        new_book_id
                    )
                    .execute(&mut *tx)
                    .await?;
                },
            }
        }

        // 引用未知书的书签 / 批注跳过并告警（脏数据防御）
        for bm in &export.bookmarks {
            let Some(&new_book_id) = id_map.get(&bm.book_id) else {
                tracing::warn!(
                    book_id = bm.book_id,
                    "import: bookmark references unknown book, skipped"
                );
                continue;
            };
            sqlx::query!(
                "INSERT INTO bookmarks (user_id, book_id, locator, percent, label, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?)",
                new_user_id,
                new_book_id,
                bm.locator,
                bm.percent,
                bm.label,
                bm.created_at,
            )
            .execute(&mut *tx)
            .await?;
        }

        for a in &export.annotations {
            let Some(&new_book_id) = id_map.get(&a.book_id) else {
                tracing::warn!(
                    book_id = a.book_id,
                    "import: annotation references unknown book, skipped"
                );
                continue;
            };
            sqlx::query!(
                "INSERT INTO annotations (user_id, book_id, kind, start_locator, end_locator, \
                 start_percent, end_percent, excerpt, content, created_at, updated_at) VALUES (?, \
                 ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                new_user_id,
                new_book_id,
                a.kind,
                a.start_locator,
                a.end_locator,
                a.start_percent,
                a.end_percent,
                a.excerpt,
                a.content,
                a.created_at,
                a.updated_at,
            )
            .execute(&mut *tx)
            .await?;
        }

        for d in &export.daily_reading_stats {
            sqlx::query!(
                "INSERT INTO daily_reading_stats (user_id, date, reading_seconds) VALUES (?, ?, ?)",
                new_user_id,
                d.date,
                d.reading_seconds,
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(new_user_id)
    }
    .await;

    let user_id = match db_result {
        Ok(id) => id,
        Err(e) => {
            for f in &files {
                let _ = tokio::fs::remove_file(&f.path).await;
            }
            return Err(e);
        },
    };

    Ok(models::ImportUserResponse {
        user_id,
        username,
        book_count: i64::try_from(files.len()).unwrap_or(i64::MAX),
    })
}

struct ExtractedBook {
    /// 已写入 book_dir 的文件路径
    path: PathBuf,
    content_hash: String,
}

/// 提取全部书籍文件到 book_dir（uuid 文件名）并计算内容哈希。
/// 任一书失败即清理本次已写文件后整体报错
fn extract_books(
    zip: &mut zip::ZipArchive<std::fs::File>,
    export: &UserExport,
    book_dir: &Path,
    max_file_size: usize,
) -> Result<Vec<ExtractedBook>, AppError> {
    let mut extracted = Vec::with_capacity(export.books.len());
    for (idx, book) in export.books.iter().enumerate() {
        match extract_one_book(zip, idx, book, book_dir, max_file_size) {
            Ok(b) => extracted.push(b),
            Err(e) => {
                for b in &extracted {
                    let _ = std::fs::remove_file(&b.path);
                }
                return Err(e);
            },
        }
    }
    Ok(extracted)
}

fn extract_one_book(
    zip: &mut zip::ZipArchive<std::fs::File>,
    idx: usize,
    book: &ExportBook,
    book_dir: &Path,
    max_file_size: usize,
) -> Result<ExtractedBook, AppError> {
    let name = book_entry_name(zip, idx, book.file.as_deref())?;
    let bytes = read_entry_limited(
        &mut zip
            .by_name(&name)
            .map_err(crate::utils::zip::zip_write_err)?,
        max_file_size,
    )?;
    if bytes.is_empty() {
        return Err(AppError::FileEmpty);
    }
    // 内容签名与声明格式不一致即拒绝（防条目被替换）
    let detected = crate::parser::detect_format(&bytes)?;
    if detected != book.format {
        return Err(AppError::ImportPackageInvalid(format!(
            "book '{}' content does not match declared format",
            book.title
        )));
    }

    let path = book_dir.join(format!("{}.{}", generate_uuid(), book.format.ext()));
    std::fs::write(&path, &bytes).map_err(AppError::Io)?;
    Ok(ExtractedBook {
        path,
        content_hash: sha256_hex(&bytes),
    })
}

/// 定位书籍文件在 zip 内的条目名：优先 metadata 的显式名，
/// 历史格式回退为 `{idx:03}_` 前缀匹配
fn book_entry_name(
    zip: &mut zip::ZipArchive<std::fs::File>,
    idx: usize,
    explicit: Option<&str>,
) -> Result<String, AppError> {
    if let Some(name) = explicit {
        if zip.by_name(name).is_err() {
            return Err(AppError::ImportPackageInvalid(format!(
                "book file entry '{name}' not found"
            )));
        }
        return Ok(name.to_string());
    }
    let prefix = format!("{idx:03}_");
    // zip 9 的 file_names() 按需做编码转换，返回 Result<Cow<str>>
    zip.file_names()
        .find_map(|n| match n {
            Ok(name) if name.starts_with(&prefix) => Some(name.into_owned()),
            _ => None,
        })
        .ok_or_else(|| {
            AppError::ImportPackageInvalid(format!(
                "book file entry with prefix '{prefix}' not found"
            ))
        })
}

/// 读取单个 zip 条目并限制解压大小（读的是用户提供的包，失败按 400 处理）
fn read_entry_limited(
    entry: &mut zip::read::ZipFile<'_, std::fs::File>,
    max: usize,
) -> Result<Vec<u8>, AppError> {
    let mut buf = Vec::new();
    entry
        .by_ref()
        .take(max as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| AppError::ImportPackageInvalid(format!("failed to read zip entry: {e}")))?;
    if buf.len() > max {
        return Err(AppError::FileTooLarge);
    }
    Ok(buf)
}

/// 导入前的完整性校验：把包内非法字段拦成 400，而非触发 DB 约束 500
fn validate_export(export: &UserExport) -> Result<(), AppError> {
    if export.version > EXPORT_FORMAT_VERSION {
        return Err(AppError::ImportPackageInvalid(format!(
            "unsupported export format version {} (supported max {})",
            export.version, EXPORT_FORMAT_VERSION
        )));
    }
    for (i, b) in export.books.iter().enumerate() {
        if b.title.trim().is_empty() {
            return Err(AppError::ImportPackageInvalid(format!(
                "books[{i}].title is empty"
            )));
        }
    }
    let percent_ok = |v: f64| (0.0..=1.0).contains(&v);
    for (i, p) in export.reading_progress.iter().enumerate() {
        if !percent_ok(p.percent) {
            return Err(AppError::ImportPackageInvalid(format!(
                "reading_progress[{i}].percent out of range"
            )));
        }
    }
    for (i, bm) in export.bookmarks.iter().enumerate() {
        if !percent_ok(bm.percent) {
            return Err(AppError::ImportPackageInvalid(format!(
                "bookmarks[{i}].percent out of range"
            )));
        }
    }
    for (i, a) in export.annotations.iter().enumerate() {
        if !percent_ok(a.start_percent) || a.end_percent.is_some_and(|v| !percent_ok(v)) {
            return Err(AppError::ImportPackageInvalid(format!(
                "annotations[{i}].percent out of range"
            )));
        }
    }
    // 章节数上限与上传路径一致
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for c in &export.chapters {
        *counts.entry(c.book_id).or_default() += 1;
    }
    if counts
        .values()
        .any(|&n| n > crate::parser::chapter::MAX_CHAPTERS)
    {
        return Err(AppError::TooManyChapters);
    }
    // 时区影响阅读统计归算，非法值拒绝（与 update_settings 同一校验）
    if let Some(s) = export.reading_settings.first()
        && s.timezone.parse::<chrono_tz::Tz>().is_err()
    {
        return Err(AppError::InvalidTimezone);
    }
    Ok(())
}

/// 文件名净化：替换路径分隔符与 Windows 保留字符
fn sanitize_filename(title: &str) -> String {
    title
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect()
}

/// 空串（或纯空白）视为未提供
fn non_empty(input: Option<String>) -> Option<String> {
    input.filter(|s| !s.trim().is_empty())
}

/// RFC 5987：把 attr-char 之外的字节做 UTF-8 percent 编码，
/// 用于 Content-Disposition 的 filename* 参数（支持中文文件名，且不会产生非法 header）。
pub(crate) fn percent_encode_filename(filename: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(filename.len());
    for byte in filename.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'!'
            | b'#'
            | b'$'
            | b'&'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~' => out.push(char::from(byte)),
            _ => write!(out, "%{byte:02X}").expect("writing to a String never fails"),
        }
    }
    out
}
