use std::str::FromStr;
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

// 中文数字映射
const CN_NUMS: [(&str, i64); 20] = [
    ("零", 0),
    ("〇", 0),
    ("一", 1),
    ("壹", 1),
    ("二", 2),
    ("贰", 2),
    ("两", 2),
    ("三", 3),
    ("叁", 3),
    ("四", 4),
    ("肆", 4),
    ("五", 5),
    ("伍", 5),
    ("六", 6),
    ("陆", 6),
    ("七", 7),
    ("柒", 7),
    ("八", 8),
    ("捌", 8),
    ("九", 9),
];

const CN_UNITS: [(&str, i64); 5] = [
    ("十", 10),
    ("拾", 10),
    ("百", 100),
    ("佰", 100),
    ("千", 1000),
];

// 生成唯一ID
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
        .map_err(|e| AppError::Internal(format!("Salt 编码失败: {}", e)))?;

    // 3. 执行 Argon2 哈希（默认使用推荐的 Argon2id 变体）
    let argon2 = Argon2::default();

    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| AppError::Internal(format!("密码哈希失败: {}", e)))
}

// 验证密码
pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    use argon2::{
        Argon2,
        password_hash::{PasswordHash, PasswordVerifier},
    };

    let parsed_hash =
        PasswordHash::new(hash).map_err(|e| AppError::Internal(format!("解析哈希失败: {}", e)))?;

    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

// 解析中文数字章节
pub fn parse_chinese_chapter_number(text: &str) -> Option<i64> {
    let text = text.trim();
    let mut result = 0;
    let mut temp = 0;
    let mut has_digit = false;

    for c in text.chars() {
        let c_str = c.to_string();

        // 查找数字
        if let Some(&(_, value)) = CN_NUMS.iter().find(|&&(s, _)| s == c_str) {
            temp = value;
            has_digit = true;
            continue;
        }

        // 查找单位
        if let Some(&(_, unit)) = CN_UNITS.iter().find(|&&(s, _)| s == c_str) {
            // 如果前面有数字，则为该数字乘以单位
            if temp > 0 {
                result += temp * unit;
            } else {
                // 否则单位前视为1（如"十一"中的"十"）
                result += unit;
            }
            temp = 0;
            continue;
        }
    }

    // 处理没有单位的情况（如末尾的个位数）
    if temp > 0 {
        result += temp;
    }

    if has_digit || result > 0 {
        Some(result)
    } else {
        None
    }
}

// 从章节标题中提取章节号
pub fn extract_chapter_number(title: &str) -> Option<i64> {
    if let Some(capture) = Regex::new(r"第\s*(\d+)\s*[章节卷集部篇]")
        .ok()?
        .captures(title)
        .ok()?
        && let Some(num_str) = capture.get(1)
    {
        return i64::from_str(num_str.as_str()).ok();
    }

    if let Some(capture) = Regex::new(
        r"第\s*([零〇一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟]+)\s*[章节卷集部篇]",
    )
    .ok()?
    .captures(title)
    .ok()?
        && let Some(num_str) = capture.get(1)
    {
        return parse_chinese_chapter_number(num_str.as_str());
    }

    None
}

// 从文本中提取章节
pub fn extract_chapters(content: &str) -> Vec<(String, usize)> {
    let mut chapters = Vec::new();
    let mut lines = content.lines().enumerate();

    if let Some((pos, first_line)) = lines.next() {
        let first_line = first_line.trim();
        if !first_line.is_empty() {
            chapters.push((first_line.to_string(), pos));
        }
    }

    for (line_num, line) in lines {
        let line = line.trim();
        if CHAPTER_RE.is_match(line).unwrap_or(false) {
            chapters.push((line.to_string(), line_num));
        }
    }

    chapters
}

/// EPUB 书籍解析结果
pub struct EpubParseResult {
    pub text_content: String,
    pub chapters: Vec<(String, usize)>,
}

/// 从 EPUB 字节数据中提取纯文本和章节信息
pub fn parse_epub(data: &[u8]) -> Result<EpubParseResult, AppError> {
    use epub::doc::EpubDoc;
    use scraper::{Html, Selector};

    let mut doc = EpubDoc::from_reader(std::io::Cursor::new(data))
        .map_err(|e| AppError::ParseError(format!("EPUB 解析失败: {}", e)))?;

    let p_selector =
        Selector::parse("p").map_err(|e| AppError::Internal(format!("CSS 选择器错误: {:?}", e)))?;
    let heading_selector = Selector::parse("h1, h2, h3, h4, h5, h6")
        .map_err(|e| AppError::Internal(format!("CSS 选择器错误: {:?}", e)))?;

    let num_chapters = doc.get_num_chapters();
    let mut full_text = String::new();
    let mut chapters: Vec<(String, usize)> = Vec::new();

    for i in 0..num_chapters {
        doc.set_current_chapter(i);
        let content = match doc.get_current_str() {
            Some((_mime, s)) => s,
            None => continue,
        };

        let html = Html::parse_document(&content);

        // 提取标题作为章节名
        let chapter_title = html
            .select(&heading_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .filter(|t| !t.is_empty());

        if let Some(title) = chapter_title {
            let pos = full_text.chars().count();
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
