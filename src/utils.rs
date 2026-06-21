use std::sync::LazyLock;

use anyhow::Result;
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

pub fn generate_uuid() -> String {
    Uuid::new_v4().to_string()
}

// 生成随机邀请码
pub fn generate_invite_code() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    const CODE_LEN: usize = 8;

    let mut rng = rand::rng();
    let code: String = (0..CODE_LEN)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();

    code
}

// 哈希密码
pub fn hash_password(password: &str) -> Result<String, AppError> {
    // 1. 使用 rand 0.10 的 API 独立生成 16 字节的随机数组。
    // rand::random() 会隐式调用底层的加密安全的随机数生成器（在 0.10 中为
    // thread-local ChaCha12）
    let salt_bytes: [u8; 16] = rand::random();

    // 2. 将随机字节数组手动编码为 Base64 格式的 SaltString，完美绕过 Trait 冲突
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|e| AppError::Internal(format!("Salt encoding failed: {e}")))?;

    // 3. 执行 Argon2 哈希（默认使用推荐的 Argon2id 变体）
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

// 从文本中提取章节
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn extract_chapters(content: &str) -> Vec<(String, i64)> {
    let mut chapters = Vec::new();
    let mut lines = content.lines().enumerate();

    if let Some((pos, first_line)) = lines.next() {
        let first_line = first_line.trim();
        if !first_line.is_empty() {
            chapters.push((first_line.to_string(), pos as i64));
        }
    }

    for (line_num, line) in lines {
        let line = line.trim();
        if CHAPTER_RE.is_match(line).unwrap_or(false) {
            chapters.push((line.to_string(), line_num as i64));
        }
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
