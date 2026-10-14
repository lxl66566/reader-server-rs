use std::sync::LazyLock;

use fancy_regex::Regex;

use crate::error::AppError;

/// 单本书章节数上限：防御构造性文本（如 50MB 全是短章节标题行）
/// 产出百万级章节的一次性巨型 INSERT
pub const MAX_CHAPTERS: usize = 50_000;

static CHAPTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[　\s]*(序章|序言|卷首语|扉页|楔子|正文(?!完|结)|终章|后记|尾声|番外|第?\s{0,4}[\d〇零一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟]+?\s{0,4}(?:章|节(?!课)|卷|集(?![合和])|部(?![分赛游])|篇(?!张))).{0,30}$"
    ).expect("invalid chapter regex")
});

/// 编译自定义章节切分正则；纯空白视为未提供（返回 None = 内置默认规则）
fn compile_pattern(pattern: &str) -> Result<Option<Regex>, AppError> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Regex::new(trimmed)
        .map(Some)
        .map_err(|e| AppError::InvalidChapterRegex(e.to_string()))
}

/// 校验自定义章节切分正则可编译；上传 / 更新 / 导入在写库前快速失败
pub fn validate_chapter_pattern(pattern: &str) -> Result<(), AppError> {
    compile_pattern(pattern).map(|_| ())
}

/// 从文本中提取章节
///
/// `pattern` 为本书的自定义章节切分正则；`None` / 纯空白时沿用内置 [`CHAPTER_RE`]。
/// 自定义正则对整行 trim 后的文本做 `is_match`，与内置规则同一坐标系。
///
/// 返回 `(章节标题, 章节首字符在全文字符流中的偏移)` 列表。
/// 偏移量统一以“字符”（`chars().count()`）为单位，与
/// `reading_progress.locator`、`bookmarks.locator`、`get_book_content`
/// 的 `position` 参数保持一致——章节跳转、内容切片才能对齐。
///
/// 章节数超过 [`MAX_CHAPTERS`] 时返回错误（视为畸形文件），不做静默截断。
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn extract_chapters(
    content: &str,
    pattern: Option<&str>,
) -> Result<Vec<(String, i64)>, AppError> {
    // 自定义 pattern 每次调用编译一次（单本书一次性操作，无需缓存）
    let custom = compile_pattern(pattern.unwrap_or(""))?;
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
            match &custom {
                Some(re) => re.is_match(trimmed),
                None => CHAPTER_RE.is_match(trimmed),
            }
            .unwrap_or(false)
        };
        if should_record {
            if chapters.len() >= MAX_CHAPTERS {
                return Err(AppError::TooManyChapters);
            }
            chapters.push((trimmed.to_string(), char_offset));
        }
        // 累加真实行宽（split_inclusive 保留了行尾换行符）
        char_offset += raw_line.chars().count() as i64;
        is_first_line = false;
    }

    Ok(chapters)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let chapters = extract_chapters(content, None).unwrap();
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].0, "序章 开始");
        assert_eq!(chapters[1].0, "第二章 进展");
        assert_offsets_point_to_titles(content, &chapters);
    }

    #[test]
    fn extract_chapters_crlf_offsets() {
        // 纯 CRLF：每行行尾占 2 个字符，偏移必须按真实行宽累加
        let content = "序章 开始\r\n一些内容\r\n第二章 进展\r\n更多内容\r\n";
        let chapters = extract_chapters(content, None).unwrap();
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
        let chapters = extract_chapters(content, None).unwrap();
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
        let chapters = extract_chapters(content, None).unwrap();
        assert_eq!(chapters.len(), 2);
        assert_offsets_point_to_titles(content, &chapters);
    }

    #[test]
    fn extract_chapters_custom_pattern_overrides_builtin() {
        // 自定义正则生效：内置规则识别不了的标题行被切出，首行特殊逻辑保留
        let content = "楔子\nSegment 01 开端\n过渡\nSegment 02 转折\n";
        let chapters = extract_chapters(content, Some(r"^Segment \d+")).unwrap();
        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].0, "楔子");
        assert_eq!(chapters[1].0, "Segment 01 开端");
        assert_eq!(chapters[2].0, "Segment 02 转折");
        assert_offsets_point_to_titles(content, &chapters);
    }

    #[test]
    fn extract_chapters_custom_pattern_crlf_offsets() {
        // 自定义正则同样必须按真实行宽（含 \r）累加偏移
        let content = "开头\r\nSEG 一\r\n正文\r\nSEG 二\r\n";
        let chapters = extract_chapters(content, Some("^SEG")).unwrap();
        assert_eq!(chapters.len(), 3);
        // "开头\r\n" = 4（含 \r）
        assert_eq!(chapters[1].1, 4);
        // "开头\r\n" + "SEG 一\r\n" + "正文\r\n" = 4 + 7 + 4 = 15
        assert_eq!(chapters[2].1, 15);
        assert_offsets_point_to_titles(content, &chapters);
    }

    #[test]
    fn extract_chapters_custom_pattern_blank_falls_back_to_builtin() {
        // 纯空白 pattern 视为未提供，回退内置规则
        let content = "序章 开始\n一些内容\n";
        let chapters = extract_chapters(content, Some("  ")).unwrap();
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].0, "序章 开始");
    }

    #[test]
    fn extract_chapters_custom_pattern_invalid_regex() {
        let err = extract_chapters("第一章\n", Some(r"[unclosed")).unwrap_err();
        assert!(
            matches!(err, AppError::InvalidChapterRegex(_)),
            "got {err:?}"
        );
        // 校验入口与提取入口同一错误
        let err = validate_chapter_pattern("(").unwrap_err();
        assert!(matches!(err, AppError::InvalidChapterRegex(_)));
        validate_chapter_pattern("^\\d+").unwrap();
        validate_chapter_pattern("").unwrap();
    }
}
