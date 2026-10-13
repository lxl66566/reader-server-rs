use crate::error::AppError;

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
}
