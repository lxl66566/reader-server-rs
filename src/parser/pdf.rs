use crate::error::AppError;

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
