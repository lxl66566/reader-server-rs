use std::sync::LazyLock;

use fancy_regex::Regex;

static CHAPTER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[　\s]*(序章|序言|卷首语|扉页|楔子|正文(?!完|结)|终章|后记|尾声|番外|第?\s{0,4}[\d〇零一二两三四五六七八九十百千万壹贰叁肆伍陆柒捌玖拾佰仟]+?\s{0,4}(?:章|节(?!课)|卷|集(?![合和])|部(?![分赛游])|篇(?!张))).{0,30}$"
    ).expect("invalid chapter regex")
});

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
