use std::sync::LazyLock;

use argon2::{Argon2, PasswordHasher as _, password_hash::SaltString};
use fancy_regex::Regex;
use rand::RngExt;
use uuid::Uuid;

use crate::error::AppError;

static CHAPTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[　\s]*(序章|序言|卷首语|扉页|楔子|正文(?!完|结)|终章|后记|尾声|番外|第?\s{0,4}[\d〇零一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟]+?\s{0,4}(?:章|节(?!课)|卷|集(?![合和])|部(?![分赛游])|篇(?!张))).{0,30}$"
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
    use sha2::{Digest, Sha256};
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
/// `reading_progress.locator`、`bookmarks.locator`、`get_book_content`
/// 的 `position` 参数保持一致——章节跳转、内容切片才能对齐。
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn extract_chapters(content: &str) -> Vec<(String, i64)> {
    let mut chapters = Vec::new();
    let mut char_offset = 0i64; // 当前行首字符在全文中的偏移
    let mut is_first_line = true;

    // 用 split_inclusive 按真实行宽（含 \n / \r\n 行尾）累加偏移，
    // 保证与存储的原始文件处于同一坐标系；lines() 会剥掉 \r 导致 CRLF 文件偏移错位
    for raw_line in content.split_inclusive('\n') {
        let line = raw_line.trim_end_matches(['\n', '\r']);
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
        // 累加真实行宽（split_inclusive 保留了行尾换行符）
        char_offset += raw_line.chars().count() as i64;
        is_first_line = false;
    }

    chapters
}

// ============ EPUB ============

/// EPUB 章节信息（与格式无关的中间结构）
#[derive(Debug, Clone)]
pub struct EpubChapter {
    /// 章节标题（取自 TOC 或首个 heading）
    pub title: String,
    /// spine 索引（0-based），与 epub.js reader.chapter(i) 对齐
    pub spine_index: i64,
    /// 该章节首字符在全文拼接序列中的字符偏移（仅供搜索边界使用）
    #[allow(dead_code)]
    pub char_offset: i64,
}

/// EPUB 解析结果
pub struct EpubParseResult {
    /// 拼接后的全文纯文本（用于搜索、章节字数）
    pub text_content: String,
    pub chapters: Vec<EpubChapter>,
    pub title: Option<String>,
    pub author: Option<String>,
}

/// 从 EPUB 字节数据中提取章节列表与拼接全文
///
/// 设计：
/// - 章节顺序按 spine 排列（与 epub.js / foliate 等渲染器一致）
/// - 每个章节的 `char_offset` 是拼接全文的字符偏移，仅供服务端搜索/边界使用
/// - `spine_index` 是前端 `EpubCFI` / 渲染时的章节序号
pub fn parse_epub(data: &[u8]) -> Result<EpubParseResult, AppError> {
    use epub::doc::EpubDoc;
    use scraper::{Html, Selector};

    let mut doc = EpubDoc::from_reader(std::io::Cursor::new(data))
        .map_err(|e| AppError::Internal(format!("EPUB parsing failed: {e}")))?;

    let p_selector = Selector::parse("p")
        .map_err(|e| AppError::Internal(format!("CSS selector error: {e:?}")))?;
    let heading_selector = Selector::parse("h1, h2, h3, h4, h5, h6")
        .map_err(|e| AppError::Internal(format!("CSS selector error: {e:?}")))?;

    let title = doc.mdata("title").map(|item| item.value.clone());
    let author = doc.mdata("creator").map(|item| item.value.clone());

    let num_chapters = doc.get_num_chapters();
    let mut full_text = String::new();
    let mut chapters: Vec<EpubChapter> = Vec::new();

    for i in 0..num_chapters {
        doc.set_current_chapter(i);
        let Some((_mime, content)) = doc.get_current_str() else {
            continue;
        };

        let html = Html::parse_document(&content);

        // 章节标题优先级：首个 heading > spine toc 标题 > "未命名章节 N"
        let chapter_title = html
            .select(&heading_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| format!("Chapter {}", i + 1));

        #[allow(clippy::cast_possible_wrap)]
        let char_offset = full_text.chars().count() as i64;
        chapters.push(EpubChapter {
            title: chapter_title,
            #[allow(clippy::cast_possible_wrap)]
            spine_index: i as i64,
            char_offset,
        });

        // 提取段落文本，用于全文搜索
        for p in html.select(&p_selector) {
            let text: String = p.text().collect::<Vec<_>>().join("").trim().to_string();
            if !text.is_empty() {
                if !full_text.is_empty() {
                    full_text.push('\n');
                }
                full_text.push_str(&text);
            }
        }
    }

    Ok(EpubParseResult {
        text_content: full_text,
        chapters,
        title,
        author,
    })
}

// ============ PDF ============

/// PDF 解析结果
pub struct PdfParseResult {
    pub num_pages: i64,
    pub title: Option<String>,
    pub author: Option<String>,
}

/// 解析 PDF 字节数据，提取页数与元数据
///
/// 使用 lopdf；只读取 Catalog 中的 Page tree 大小与 Info 字典，
/// 不渲染内容（PDF 渲染由前端 pdf.js 完成）
pub fn parse_pdf(data: &[u8]) -> Result<PdfParseResult, AppError> {
    use lopdf::Document;

    let doc = Document::load_mem(data)
        .map_err(|e| AppError::Internal(format!("PDF parsing failed: {e}")))?;

    let num_pages = doc.get_pages().len();

    // 从 Info 字典读取标题/作者；忽略解析失败（容错优先）
    let (title, author) = doc
        .trailer
        .get(b"Info")
        .and_then(lopdf::Object::as_reference)
        .and_then(|id| doc.get_object(id))
        .and_then(lopdf::Object::as_dict)
        .map_or((None, None), |dict| {
            let title = pdf_string(dict.get(b"Title").ok());
            let author = pdf_string(dict.get(b"Author").ok());
            (title, author)
        });

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap
    )]
    Ok(PdfParseResult {
        num_pages: num_pages as i64,
        title,
        author,
    })
}

/// 从 PDF Object 取出字符串值（处理 Hexadecimal/Literal 两种编码）
fn pdf_string(obj: Option<&lopdf::Object>) -> Option<String> {
    use lopdf::{Object, StringFormat};
    let obj = obj?;
    match obj {
        Object::String(bytes, fmt) => match fmt {
            StringFormat::Literal => {
                // PDFDocEncoding 子集 ≈ Latin-1；常见 ASCII 文本可直读
                String::from_utf8(bytes.clone())
                    .ok()
                    .or_else(|| Some(iso8859_1_to_string(bytes)))
            },
            StringFormat::Hexadecimal => String::from_utf8(bytes.clone()).ok(),
        },
        Object::Name(bytes) => String::from_utf8(bytes.clone()).ok(),
        _ => None,
    }
}

/// 将 Latin-1 (ISO-8859-1) 字节序列解释为字符串
/// PDF 文本字符串常用 Latin-1 编码（非 UTF-8），此为兜底解码
fn iso8859_1_to_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

// ============ 文件类型检测 ============

/// 检测上传文件的格式（基于内容签名，不依赖扩展名）
///
/// 检测顺序：
/// 1. `%PDF-` 前缀 → PDF
/// 2. infer 识别 `application/epub+zip` → EPUB
/// 3. UTF-8 解码成功 → TXT
/// 4. 否则 → 不支持
pub fn detect_format(content: &[u8]) -> Result<crate::models::BookFormat, AppError> {
    // PDF 是文本签名（%PDF-），infer crate 不识别，需手工判断
    if content.starts_with(b"%PDF-") {
        return Ok(crate::models::BookFormat::Pdf);
    }

    if let Some(kind) = infer::get(content) {
        if kind.mime_type() == "application/epub+zip" {
            return Ok(crate::models::BookFormat::Epub);
        }
        // 其它二进制格式（exe、docx 等）一律拒绝
        return Err(AppError::UnsupportedFileFormat);
    }

    // infer 无法识别（通常是纯文本）：尝试 UTF-8 解码
    if std::str::from_utf8(content).is_ok() {
        Ok(crate::models::BookFormat::Txt)
    } else {
        Err(AppError::FileInvalidUtf8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_format_pdf() {
        let pdf = b"%PDF-1.4\n...";
        assert_eq!(detect_format(pdf).unwrap(), crate::models::BookFormat::Pdf);
    }

    #[test]
    fn detect_format_txt() {
        let txt = "中文测试 content".as_bytes();
        assert_eq!(detect_format(txt).unwrap(), crate::models::BookFormat::Txt);
    }

    #[test]
    fn detect_format_invalid_utf8() {
        let bad = &[0xff, 0xfe, 0xfd];
        assert!(detect_format(bad).is_err());
    }

    #[test]
    fn detect_format_exe_rejected() {
        let exe: &[u8] = &[0x4d, 0x5a, 0x90, 0x00];
        assert!(detect_format(exe).is_err());
    }

    /// 校验章节偏移能正确指向原文中标题的起始位置
    fn assert_offsets_point_to_titles(content: &str, chapters: &[(String, i64)]) {
        let chars: Vec<char> = content.chars().collect();
        for (title, offset) in chapters {
            let offset = usize::try_from(*offset).unwrap();
            let actual: String = chars[offset..offset + title.chars().count()]
                .iter()
                .collect();
            assert_eq!(&actual, title, "offset {offset} does not point to title");
        }
    }

    #[test]
    fn extract_chapters_lf_offsets() {
        let content = "序章 开始\n一些内容\n第二章 进展\n更多内容\n";
        let chapters = extract_chapters(content);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].0, "序章 开始");
        assert_eq!(chapters[1].0, "第二章 进展");
        assert_offsets_point_to_titles(content, &chapters);
    }

    #[test]
    fn extract_chapters_crlf_offsets() {
        // 纯 CRLF：每行行尾占 2 个字符，偏移必须按真实行宽累加
        let content = "序章 开始\r\n一些内容\r\n第二章 进展\r\n更多内容\r\n";
        let chapters = extract_chapters(content);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].1, 0);
        // "序章 开始\r\n正文内容\r\n" = (5 + 2) + (4 + 2) = 13
        assert_eq!(chapters[1].1, 13);
        assert_offsets_point_to_titles(content, &chapters);
    }

    #[test]
    fn extract_chapters_mixed_crlf_lf_offsets() {
        // CRLF 与 LF 混合：偏移同样必须按真实行宽累加
        let content = "line1\r\n第二章 abc\n第三章 def\r\n";
        let chapters = extract_chapters(content);
        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].1, 0);
        // "line1\r\n" = 7
        assert_eq!(chapters[1].1, 7);
        // "line1\r\n" + "第二章 abc\n" = 7 + 8 = 15
        assert_eq!(chapters[2].1, 15);
        assert_offsets_point_to_titles(content, &chapters);
    }

    #[test]
    fn extract_chapters_no_trailing_newline() {
        // 末尾无换行符时偏移也不应被多算
        let content = "第一章 开头\n内容\n第二章 结尾";
        let chapters = extract_chapters(content);
        assert_eq!(chapters.len(), 2);
        assert_offsets_point_to_titles(content, &chapters);
    }
}
