use std::sync::LazyLock;

use argon2::{Argon2, PasswordHasher as _, password_hash::SaltString};
use fancy_regex::Regex;
use rand::RngExt;
use uuid::Uuid;

use crate::error::AppError;

static CHAPTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[　\s](序章|序言|卷首语|扉页|楔子|正文(?!完|结)|终章|后记|尾声|番外|第?\s{0,4}[\d〇零一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟]+?\s{0,4}(?:章|节(?!课)|卷|集(?![合和])|部(?![分赛游])|篇(?!张))).{0,30}$"
    ).expect("invalid chapter regex")
});

/// 简易 `ETag` 哈希：用 FNV-1a 64bit，足够短、足够快、无外部依赖
/// 返回带双引号的 `ETag` 字符串（符合 RFC 7232）
pub fn make_etag(input: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x100_0000_01b3;

    let mut hash = FNV_OFFSET;
    for &byte in input.as_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    // 加 W/ 前缀表示 weak ETag：内容语义相同即可，不需要严格 byte-equal
    format!("W/\"{hash:016x}\"")
}

pub fn generate_uuid() -> String {
    Uuid::new_v4().to_string()
}

/// 计算数据的 SHA-256 hex（小写）
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    // hex(lower)
    let mut out = String::with_capacity(64);
    for b in result {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

// 生成随机邀请码
pub fn generate_invite_code() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    const CODE_LEN: usize = 8;

    let mut rng = rand::rng();
    (0..CODE_LEN)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

// 哈希密码
pub fn hash_password(password: &str) -> Result<String, AppError> {
    let mut rng = rand::rng();
    // 生成 16 字节的随机 salt（使用 thread-local ChaCha12 RNG）
    let salt_bytes: [u8; 16] = rng.random();

    // 将随机字节数组编码为 Base64 格式的 SaltString
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|e| AppError::Internal(format!("Salt encoding failed: {e}")))?;

    // 执行 Argon2 哈希（默认使用推荐的 Argon2id 变体）
    let argon2 = Argon2::default();

    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| AppError::Internal(format!("Password hashing failed: {e}")))
}

// 验证密码
pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    use argon2::{
        Argon2,
        password_hash::{PasswordHash, PasswordVerifier},
    };

    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| AppError::Internal(format!("Failed to parse hash: {e}")))?;

    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

/// 从文本中提取章节
///
/// 返回 `(章节标题, 章节首字符在全文字符流中的偏移)` 列表。
/// 偏移量统一以"字符"（`chars().count()`）为单位，与
/// `reading_progress.position`、`bookmarks.position`、`get_book_content`
/// 的 `position` 参数保持一致——章节跳转、内容切片才能对齐。
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn extract_chapters(content: &str) -> Vec<(String, i64)> {
    let mut chapters = Vec::new();
    let mut char_offset = 0i64; // 当前行首字符在全文中的偏移
    let mut is_first_line = true;

    for line in content.lines() {
        let line_char_count = line.chars().count() as i64;
        let trimmed = line.trim();
        let should_record = if is_first_line {
            // 第一行非空时直接作为首章，无论是否匹配章节正则
            !trimmed.is_empty()
        } else {
            CHAPTER_RE.is_match(trimmed).unwrap_or(false)
        };
        if should_record {
            chapters.push((trimmed.to_string(), char_offset));
        }
        // 跳过本行字符 + 1 个换行符（lines() 不返回行尾，但全文里有一个）
        char_offset += line_char_count + 1;
        is_first_line = false;
    }

    chapters
}

/// EPUB 书籍解析结果
pub struct EpubParseResult {
    pub text_content: String,
    pub chapters: Vec<(String, i64)>,
}

/// 从 EPUB 字节数据中提取纯文本和章节信息
pub fn parse_epub(data: &[u8]) -> Result<EpubParseResult, AppError> {
    use epub::doc::EpubDoc;
    use scraper::{Html, Selector};

    let mut doc = EpubDoc::from_reader(std::io::Cursor::new(data))
        .map_err(|e| AppError::Internal(format!("EPUB parsing failed: {e}")))?;

    let p_selector = Selector::parse("p")
        .map_err(|e| AppError::Internal(format!("CSS selector error: {e:?}")))?;
    let heading_selector = Selector::parse("h1, h2, h3, h4, h5, h6")
        .map_err(|e| AppError::Internal(format!("CSS selector error: {e:?}")))?;

    let num_chapters = doc.get_num_chapters();
    let mut full_text = String::new();
    let mut chapters: Vec<(String, i64)> = Vec::new();

    for i in 0..num_chapters {
        doc.set_current_chapter(i);
        let Some((_mime, content)) = doc.get_current_str() else {
            continue;
        };

        let html = Html::parse_document(&content);

        // 提取标题作为章节名
        let chapter_title = html
            .select(&heading_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .filter(|t| !t.is_empty());

        if let Some(title) = chapter_title {
            #[allow(clippy::cast_possible_wrap)]
            let pos = full_text.chars().count() as i64;
            chapters.push((title, pos));
        }

        // 提取段落文本
        let mut chapter_parts: Vec<String> = Vec::new();
        for p in html.select(&p_selector) {
            let text: String = p.text().collect::<Vec<_>>().join("").trim().to_string();
            if !text.is_empty() {
                chapter_parts.push(text);
            }
        }

        if !chapter_parts.is_empty() {
            if !full_text.is_empty() {
                full_text.push('\n');
            }
            full_text.push_str(&chapter_parts.join("\n"));
        }
    }

    // 如果 EPUB 的 TOC 没有提供章节，尝试用正则从文本中提取
    if chapters.is_empty() {
        chapters = extract_chapters(&full_text);
    }

    Ok(EpubParseResult {
        text_content: full_text,
        chapters,
    })
}
