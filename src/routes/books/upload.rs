use std::path::PathBuf;

use axum::{
    Json,
    extract::{State, multipart::Multipart},
};
use sqlx::{Pool, Sqlite};
use tokio::{fs, io::AsyncWriteExt};

use super::helpers::map_multipart_err;
use crate::{
    auth::AuthUser,
    config::Config,
    error::{ApiResponse, AppError},
    limits::BOOK_DESCRIPTION_MAX_CHARS,
    models::{BookFormat, ChapterResponse, UploadBookResponse},
    parser::{detect_format, extract_chapters, parse_epub, parse_pdf, validate_chapter_pattern},
    utils::{generate_uuid, sha256_hex},
};

// 上传书籍

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
                // 章节：书签 outline 的 position 为 0-based 页码（前端按页跳转）
                let chs = parsed.chapters;
                let title = if title.trim().is_empty() {
                    parsed.title.unwrap_or(title)
                } else {
                    title
                };
                (
                    0i64,
                    parsed.num_pages,
                    chs,
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

pub(super) async fn upload_book(
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

#[cfg(test)]
mod tests {
    use super::parse_upload_sync;
    use crate::{error::AppError, models::BookFormat};

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
}
