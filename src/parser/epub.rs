use std::{borrow::Cow, collections::HashMap, sync::LazyLock};

use fancy_regex::Regex;

use crate::error::AppError;

/// EPUB 章节信息（与格式无关的中间结构）
#[derive(Debug, Clone)]
pub struct EpubChapter {
    /// 章节标题（取自 TOC 或首个 heading）
    pub title: String,
    /// spine 索引（0-based），与 epub.js reader.chapter(i) 对齐
    pub spine_index: i64,
    /// 该章节首字符在全文拼接序列中的字符偏移（供书内搜索定位章节）
    pub char_offset: i64,
}

/// EPUB 解析结果
pub struct EpubParseResult {
    /// 拼接后的全文纯文本（用于搜索、章节字数）
    pub text_content: String,
    pub chapters: Vec<EpubChapter>,
    pub title: Option<String>,
    pub author: Option<String>,
    /// 内嵌元数据中的简介（dc:description）；原始文本，不做 HTML 清洗
    pub description: Option<String>,
}

/// EPUB 单个 zip 条目（章节 HTML 等）解压后的字节数上限，防御 zip 炸弹
const MAX_EPUB_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

/// EPUB 解析后拼接全文的字符数上限（20M 字符，远超正常书籍体量）
const MAX_EPUB_TOTAL_CHARS: usize = 20_000_000;

/// 自闭合的 raw-text 标签（`<script src="x"/>` 等）
///
/// XHTML 允许任意元素自闭合，但 HTML 解析器（epub.js 渲染、scraper 提取）对
/// script/style/textarea/title 这类 raw-text 元素不认自闭合：标签会保持打开，
/// 其后的整个文档都被当作原始文本吞掉，章节渲染为空白、正文提取为空
static SELF_CLOSED_RAW_TEXT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)<(script|style|textarea|title)\b([^>]*?)/>")
        .expect("invalid self-closed raw-text tag regex")
});

/// 把自闭合的 raw-text 标签展开成空标签对，恢复其 HTML 语义
fn expand_self_closed_raw_text(content: &str) -> Cow<'_, str> {
    SELF_CLOSED_RAW_TEXT_RE.replace_all(content, "<$1$2></$1>")
}

/// 归一化 zip 内路径：去 `.` 段、折叠 `..` 段
///
/// epub crate 按 OPF 目录直接拼接 href 不做归一化；资源在 OPF 上层时
/// （如 `OPS/../titlepage.xhtml`）按原样查 zip 条目会取不到
fn normalize_zip_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {},
            ".." => {
                parts.pop();
            },
            _ => parts.push(seg),
        }
    }
    parts.join("/")
}

/// 检查 epub（zip）每个条目的真实解压大小，超限直接拒绝。
/// 采用流式截断读取计数，不依赖 zip 头中可被伪造的声明大小。
fn check_epub_entry_sizes(data: &[u8], max_entry_bytes: u64) -> Result<(), AppError> {
    use std::io::Read as _;

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(data))
        .map_err(|e| AppError::FileParseError(format!("invalid EPUB zip: {e}")))?;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| AppError::FileParseError(format!("invalid EPUB zip entry: {e}")))?;
        // 多读 1 字节用于判断是否超限；io::copy 是流式的，不会把超限内容全量载入内存
        let mut limited = entry.take(max_entry_bytes + 1);
        let copied = std::io::copy(&mut limited, &mut std::io::sink())
            .map_err(|e| AppError::FileParseError(format!("corrupted EPUB entry: {e}")))?;
        if copied > max_entry_bytes {
            return Err(AppError::EpubContentTooLarge);
        }
    }
    Ok(())
}

/// 从 EPUB 字节数据中提取章节列表与拼接全文
///
/// 设计：
/// - 章节顺序按 spine 排列（与 epub.js / foliate 等渲染器一致）
/// - 每个章节的 `char_offset` 是拼接全文的字符偏移，仅供服务端搜索/边界使用
/// - `spine_index` 是前端 `EpubCFI` / 渲染时的章节序号
/// - 防御 zip 炸弹：单条目解压大小与全文总字符数均有上限，超限拒绝
pub fn parse_epub(data: &[u8]) -> Result<EpubParseResult, AppError> {
    parse_epub_with_limits(data, MAX_EPUB_ENTRY_BYTES, MAX_EPUB_TOTAL_CHARS)
}

/// 可参数化上限的内部实现，便于单测构造超小上限验证拒绝逻辑
fn parse_epub_with_limits(
    data: &[u8],
    max_entry_bytes: u64,
    max_total_chars: usize,
) -> Result<EpubParseResult, AppError> {
    use epub::doc::EpubDoc;
    use scraper::{Html, Selector};

    // 先拒绝包含超大解压条目的炸弹文件，再交给 epub crate 解析
    check_epub_entry_sizes(data, max_entry_bytes)?;

    let mut doc = EpubDoc::from_reader(std::io::Cursor::new(data))
        .map_err(|e| AppError::FileParseError(format!("invalid EPUB structure: {e}")))?;

    let p_selector = Selector::parse("p")
        .map_err(|e| AppError::Internal(format!("CSS selector error: {e:?}")))?;
    let heading_selector = Selector::parse("h1, h2, h3, h4, h5, h6")
        .map_err(|e| AppError::Internal(format!("CSS selector error: {e:?}")))?;

    let title = doc.mdata("title").map(|item| item.value.clone());
    let author = doc.mdata("creator").map(|item| item.value.clone());
    let description = doc
        .mdata("description")
        .map(|item| item.value.trim().to_string())
        .filter(|d| !d.is_empty());

    let num_chapters = doc.get_num_chapters();
    // TOC 标签按归一化路径索引，作为无 heading 章节的标题回退
    let toc_titles: HashMap<String, String> = doc
        .toc
        .iter()
        .map(|t| {
            (
                normalize_zip_path(&t.content.to_string_lossy().replace('\\', "/")),
                t.label.clone(),
            )
        })
        .collect();
    let mut full_text = String::new();
    let mut chapters: Vec<EpubChapter> = Vec::new();
    // 全文累计字符数（含拼接换行符），用于章节偏移与总量上限检查
    let mut total_chars: usize = 0;

    for i in 0..num_chapters {
        doc.set_current_chapter(i);
        let current_path = doc
            .get_current_path()
            .map(|p| normalize_zip_path(&p.to_string_lossy().replace('\\', "/")));
        // 先按 spine id 取内容；取不到时（路径含 `..` 未归一化等）按归一化路径回退
        let content = doc
            .get_current_str()
            .map(|(content, _)| content)
            .or_else(|| doc.get_resource_str_by_path(current_path.as_deref()?));
        let Some(content) = content else {
            continue;
        };

        let html = Html::parse_document(&expand_self_closed_raw_text(&content));

        // 章节标题优先级：首个 heading > TOC 标签 > "Chapter N"
        let chapter_title = html
            .select(&heading_selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .filter(|t| !t.is_empty())
            .or_else(|| {
                current_path
                    .as_deref()
                    .and_then(|p| toc_titles.get(p).cloned())
            })
            .unwrap_or_else(|| format!("Chapter {}", i + 1));

        #[allow(clippy::cast_possible_wrap)]
        let char_offset = total_chars as i64;
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
                    total_chars += 1;
                }
                total_chars += text.chars().count();
                // 拼接全文超过字符数上限：视为炸弹/异常文件，拒绝入库
                if total_chars > max_total_chars {
                    return Err(AppError::EpubContentTooLarge);
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
        description,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个最小可用的 EPUB（zip）用于测试
    /// `manifest_href` 与 `zip_entry` 控制章节文件相对 OPF 的引用与 zip 内路径
    fn build_test_epub_opts(manifest_href: &str, zip_entry: &str, chapter_html: &str) -> Vec<u8> {
        use std::io::Write as _;

        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            let deflated = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            // epub 规范要求 mimetype 为首个条目且不压缩
            writer.start_file("mimetype", stored).unwrap();
            writer.write_all(b"application/epub+zip").unwrap();
            writer
                .start_file("META-INF/container.xml", deflated)
                .unwrap();
            writer
                .write_all(
                    br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
                )
                .unwrap();
            writer.start_file("OEBPS/content.opf", deflated).unwrap();
            writer
                .write_all(
                    format!(
                        r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title><dc:creator>Tester</dc:creator>
  </metadata>
  <manifest><item id="ch1" href="{manifest_href}" media-type="application/xhtml+xml"/></manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"#
                    )
                    .as_bytes(),
                )
                .unwrap();
            writer.start_file(zip_entry, deflated).unwrap();
            writer.write_all(chapter_html.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    /// 构造章节文件位于 OEBPS/ 内的最小 EPUB
    fn build_test_epub(chapter_html: &str) -> Vec<u8> {
        build_test_epub_opts("ch1.xhtml", "OEBPS/ch1.xhtml", chapter_html)
    }

    #[test]
    fn parse_epub_basic() {
        let epub_data = build_test_epub(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>
<h1>第一章 开始</h1><p>第一段内容</p><p>第二段内容</p>
</body></html>"#,
        );
        let result = parse_epub(&epub_data).unwrap();
        assert_eq!(result.title.as_deref(), Some("Test Book"));
        assert_eq!(result.author.as_deref(), Some("Tester"));
        assert_eq!(result.chapters.len(), 1);
        assert_eq!(result.chapters[0].title, "第一章 开始");
        assert!(result.text_content.contains("第一段内容"));
    }

    #[test]
    fn parse_epub_expands_self_closed_raw_text() {
        // XHTML 自闭合的 script：HTML 解析器会把其后整份文档当脚本文本吞掉，
        // 不展开则章节标题与正文均解析为空
        let epub_data = build_test_epub(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><script src="insertSpan.js" type="text/javascript"/></head><body>
<h1>第一章 开始</h1><p>第一段内容</p>
</body></html>"#,
        );
        let result = parse_epub(&epub_data).unwrap();
        assert_eq!(result.chapters.len(), 1);
        assert_eq!(result.chapters[0].title, "第一章 开始");
        assert!(result.text_content.contains("第一段内容"));
    }

    #[test]
    fn parse_epub_resolves_parent_relative_spine_paths() {
        // 章节文件在 OPF 上层目录（href="../ch1.xhtml"）：epub crate 按
        // `OEBPS/../ch1.xhtml` 查 zip 取不到，需按归一化路径回退
        let epub_data = build_test_epub_opts(
            "../ch1.xhtml",
            "ch1.xhtml",
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>第一章</h1><p>正文内容</p></body></html>"#,
        );
        let result = parse_epub(&epub_data).unwrap();
        assert_eq!(result.chapters.len(), 1);
        assert_eq!(result.chapters[0].title, "第一章");
        assert!(result.text_content.contains("正文内容"));
    }

    #[test]
    fn parse_epub_rejects_oversized_entry() {
        // 单条目解压后超过 max_entry_bytes：拒绝
        let big_body = "x".repeat(8192);
        let epub_data = build_test_epub(&format!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>{big_body}</p></body></html>"#
        ));
        let result = parse_epub_with_limits(&epub_data, 1024, MAX_EPUB_TOTAL_CHARS);
        assert!(result.is_err());
        // 放宽条目上限后应可正常解析（拒绝确实由条目大小触发）
        assert!(parse_epub_with_limits(&epub_data, 1024 * 1024, MAX_EPUB_TOTAL_CHARS).is_ok());
    }

    #[test]
    fn parse_epub_rejects_excessive_total_chars() {
        // 拼接全文字符数超过 max_total_chars：拒绝
        let epub_data = build_test_epub(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>
<h1>第一章</h1><p>这是一段足够长的中文文本用于触发字符数上限检查</p>
</body></html>"#,
        );
        let result = parse_epub_with_limits(&epub_data, MAX_EPUB_ENTRY_BYTES, 10);
        assert!(result.is_err());
        // 上限放宽后应可正常解析
        assert!(parse_epub_with_limits(&epub_data, MAX_EPUB_ENTRY_BYTES, usize::MAX).is_ok());
    }
}
