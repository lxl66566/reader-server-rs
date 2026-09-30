use crate::error::AppError;

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

/// EPUB 单个 zip 条目（章节 HTML 等）解压后的字节数上限，防御 zip 炸弹
const MAX_EPUB_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

/// EPUB 解析后拼接全文的字符数上限（20M 字符，远超正常书籍体量）
const MAX_EPUB_TOTAL_CHARS: usize = 20_000_000;

/// 检查 epub（zip）每个条目的真实解压大小，超限直接拒绝。
/// 采用流式截断读取计数，不依赖 zip 头中可被伪造的声明大小。
fn check_epub_entry_sizes(data: &[u8], max_entry_bytes: u64) -> Result<(), AppError> {
    use std::io::Read as _;

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(data))
        .map_err(|e| AppError::Internal(format!("EPUB zip reading failed: {e}")))?;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| AppError::Internal(format!("EPUB zip entry reading failed: {e}")))?;
        // 多读 1 字节用于判断是否超限；io::copy 是流式的，不会把超限内容全量载入内存
        let mut limited = entry.take(max_entry_bytes + 1);
        let copied = std::io::copy(&mut limited, &mut std::io::sink())
            .map_err(|e| AppError::Internal(format!("EPUB entry decompress failed: {e}")))?;
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
    // 全文累计字符数（含拼接换行符），用于章节偏移与总量上限检查
    let mut total_chars: usize = 0;

    for i in 0..num_chapters {
        doc.set_current_chapter(i);
        // epub crate 返回 (content, mime)，注意不要写反
        let Some((content, _mime)) = doc.get_current_str() else {
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个最小可用的 EPUB（zip）用于测试；chapter_html 为单章正文 HTML
    fn build_test_epub(chapter_html: &str) -> Vec<u8> {
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
                    br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Book</dc:title><dc:creator>Tester</dc:creator>
  </metadata>
  <manifest><item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest>
  <spine><itemref idref="ch1"/></spine>
</package>"#,
                )
                .unwrap();
            writer.start_file("OEBPS/ch1.xhtml", deflated).unwrap();
            writer.write_all(chapter_html.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        cursor.into_inner()
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
