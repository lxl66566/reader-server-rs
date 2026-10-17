use std::collections::HashMap;

use crate::error::AppError;

/// PDF 解析结果
pub struct PdfParseResult {
    pub num_pages: i64,
    pub title: Option<String>,
    pub author: Option<String>,
    /// 书签 outline 提取的章节：(标题, 0-based 页码)
    pub chapters: Vec<(String, i64)>,
}

/// outline 遍历上限：条目数 / 节点深度，防御病态结构的无限循环
const OUTLINE_MAX_ITEMS: usize = 2000;
const OUTLINE_MAX_DEPTH: u32 = 16;

/// 解析 PDF 字节数据，提取页数、元数据与书签 outline
///
/// 使用 lopdf；只读取 Catalog 中的 Page tree、Info 字典与 Outlines，
/// 不渲染内容（PDF 渲染由前端 pdf.js 完成）
pub fn parse_pdf(data: &[u8]) -> Result<PdfParseResult, AppError> {
    use lopdf::Document;

    let doc = Document::load_mem(data)
        .map_err(|e| AppError::FileParseError(format!("invalid PDF structure: {e}")))?;

    let pages = doc.get_pages();
    let num_pages = pages.len();

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

    let chapters = extract_outline(&doc, &pages);

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap
    )]
    Ok(PdfParseResult {
        num_pages: num_pages as i64,
        title,
        author,
        chapters,
    })
}

/// 从 Catalog 的 Outlines 提取书签章节，position 为目标页的 0-based 页码。
/// 无书签或目标页无法解析的条目跳过；只处理显式 /Dest（直连或 GoTo action），
/// 命名目标（/Names 树）不做解析。
fn extract_outline(
    doc: &lopdf::Document,
    pages: &std::collections::BTreeMap<u32, lopdf::ObjectId>,
) -> Vec<(String, i64)> {
    // 页对象 id → 0-based 页码
    let page_index: HashMap<lopdf::ObjectId, i64> = pages
        .iter()
        .map(|(no, id)| (*id, i64::from(*no) - 1))
        .collect();

    let mut out = Vec::new();
    let root = doc
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(b"Outlines").ok())
        .and_then(|obj| doc.dereference(obj).ok())
        .and_then(|(_, obj)| obj.as_dict().ok().cloned());
    let Some(root) = root else {
        return out;
    };
    // 根节点只有 First/Next 链，无标题；从顶层 First 开始按深度遍历
    if let Some(first) = dict_ref(&root, b"First") {
        walk_outline(doc, first, &page_index, 0, &mut out);
    }
    out
}

/// 读取字典中引用型键指向的 ObjectId
fn dict_ref(dict: &lopdf::Dictionary, key: &[u8]) -> Option<lopdf::ObjectId> {
    dict.get(key).ok().and_then(|obj| obj.as_reference().ok())
}

/// 遍历以 `node` 为起点的同级 Next 链；`depth` 为树深
fn walk_outline(
    doc: &lopdf::Document,
    node: lopdf::ObjectId,
    page_index: &HashMap<lopdf::ObjectId, i64>,
    depth: u32,
    out: &mut Vec<(String, i64)>,
) {
    if depth > OUTLINE_MAX_DEPTH || out.len() >= OUTLINE_MAX_ITEMS {
        return;
    }
    let mut current = Some(node);
    let mut visited = 0usize;
    while let Some(id) = current {
        visited += 1;
        if visited > OUTLINE_MAX_ITEMS || out.len() >= OUTLINE_MAX_ITEMS {
            return;
        }
        let Ok(dict) = doc.get_object(id).and_then(lopdf::Object::as_dict).cloned() else {
            return;
        };
        if let Some(title) = pdf_string(dict.get(b"Title").ok()).filter(|t| !t.is_empty())
            && let Some(page) = outline_dest_page(doc, &dict, page_index)
        {
            out.push((title, page));
        }
        // 先深后广：保持与阅读器目录一致的层级展开顺序
        if let Some(child) = dict_ref(&dict, b"First") {
            walk_outline(doc, child, page_index, depth + 1, out);
        }
        current = dict_ref(&dict, b"Next");
    }
}

/// 解析书签条目的目标页码：支持 /Dest 直连数组与 /A GoTo action，
/// 命名目标（Name/String）不解析
fn outline_dest_page(
    doc: &lopdf::Document,
    dict: &lopdf::Dictionary,
    page_index: &HashMap<lopdf::ObjectId, i64>,
) -> Option<i64> {
    use lopdf::Object;

    let target = dict.get(b"Dest").or_else(|_| dict.get(b"A")).ok()?;
    let (_, resolved) = doc.dereference(target).ok()?;
    // /A 的值是 action 字典，跳转目标数组在其 /D 键下
    let array_obj = match resolved {
        Object::Array(_) => resolved,
        Object::Dictionary(d) => d.get(b"D").ok()?,
        _ => return None,
    };
    let (_, array) = doc.dereference(array_obj).ok()?;
    // 目标数组首元素为页对象引用
    let page_ref = array.as_array().ok()?.first()?.as_reference().ok()?;
    page_index.get(&page_ref).copied()
}

/// 从 PDF Object 取出字符串值（处理 Hexadecimal/Literal 两种编码）
fn pdf_string(obj: Option<&lopdf::Object>) -> Option<String> {
    use lopdf::Object;
    let obj = obj?;
    match obj {
        Object::String(bytes, _fmt) => decode_pdf_text(bytes),
        // Literal 与 Hexadecimal 只是源文件写法差异，解码后字节一致
        Object::Name(bytes) => String::from_utf8(bytes.clone()).ok(),
        _ => None,
    }
}

/// PDF 文本字符串解码：BOM 声明的 UTF-16 优先，其次 UTF-8，兜底 Latin-1
/// （PDFDocEncoding 的可见字符段与 Latin-1 基本一致）。
/// 中文 PDF 的标题/书签普遍存为 UTF-16BE(\xfe\xff) 字节串，
/// 仅按 UTF-8 解会失败、按 Latin-1 解会得到乱码。
fn decode_pdf_text(bytes: &[u8]) -> Option<String> {
    match bytes {
        [0xfe, 0xff, rest @ ..] => {
            return Some(decode_utf16(rest, true));
        },
        [0xff, 0xfe, rest @ ..] => {
            return Some(decode_utf16(rest, false));
        },
        _ => {},
    }
    String::from_utf8(bytes.to_vec())
        .ok()
        .or_else(|| Some(bytes.iter().map(|&b| b as char).collect()))
}

/// UTF-16 解码（big_endian 选字节序）；非法代理对以 U+FFFD 替换
fn decode_utf16(bytes: &[u8], big_endian: bool) -> String {
    #[allow(clippy::cast_possible_truncation)]
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| {
            if big_endian {
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                u16::from_le_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::{decode_pdf_text, parse_pdf};
    use crate::error::AppError;

    #[test]
    fn decode_pdf_text_utf16be_bom() {
        // 「一个」的 UTF-16BE + BOM
        let bytes = [0xfe, 0xff, 0x4e, 0x00, 0x4e, 0x2a];
        assert_eq!(decode_pdf_text(&bytes).as_deref(), Some("一个"));
    }

    #[test]
    fn decode_pdf_text_utf16le_bom() {
        let bytes = [0xff, 0xfe, 0x00, 0x4e, 0x2a, 0x4e];
        assert_eq!(decode_pdf_text(&bytes).as_deref(), Some("一个"));
    }

    #[test]
    fn decode_pdf_text_utf8_and_latin1_fallback() {
        assert_eq!(decode_pdf_text("café".as_bytes()).as_deref(), Some("café"));
        // 非 UTF-8 字节序列按 Latin-1 兜底（0xE9 → é）
        assert_eq!(
            decode_pdf_text(&[0x63, 0x61, 0x66, 0xe9]).as_deref(),
            Some("caf\u{e9}")
        );
    }

    #[test]
    fn decode_pdf_text_utf16be_truncated_len() {
        // 奇数长度：末尾残缺码元被丢弃而不是 panic
        let bytes = [0xfe, 0xff, 0x4e, 0x00, 0x00];
        assert_eq!(decode_pdf_text(&bytes).as_deref(), Some("一"));
    }

    /// 用 lopdf 构造带 Info（UTF-16BE 标题）与两层 outline 的最小 PDF
    fn build_pdf() -> Vec<u8> {
        use lopdf::{Dictionary as Dict, Object, StringFormat};

        let utf16be = |s: &str| {
            let mut bytes = vec![0xfe, 0xff];
            for unit in s.encode_utf16() {
                bytes.extend_from_slice(&unit.to_be_bytes());
            }
            Object::String(bytes, StringFormat::Literal)
        };

        let mut doc = lopdf::Document::with_version("1.5");
        // 占位拿 id，字典后补（pages 需先有 kids、item0 需先有 next/first）
        let pages_id = doc.new_object_id();
        let outlines_id = doc.new_object_id();

        let mut page_dicts = Vec::new();
        let mut page_ids = Vec::new();
        for _ in 0..3 {
            let content_id = doc.add_object(lopdf::Stream::new(Dict::new(), b"BT ET".to_vec()));
            let page_dict = Dict::from_iter([
                ("Type", Object::Name(b"Page".to_vec())),
                ("Parent", pages_id.into()),
                (
                    "Resources",
                    Dict::from_iter([("Font", Dict::new().into())]).into(),
                ),
                ("Contents", content_id.into()),
            ]);
            let id = doc.add_object(page_dict);
            page_ids.push(id);
            page_dicts.push(id);
        }
        doc.set_object(
            pages_id,
            Dict::from_iter([
                ("Type", Object::Name(b"Pages".to_vec())),
                ("Count", Object::Integer(3)),
                (
                    "Kids",
                    Object::Array(page_dicts.into_iter().map(Object::Reference).collect()),
                ),
            ]),
        );

        // outline：顶层两项（第 0/2 页），第一项下挂一个子项（第 1 页）。
        // item0 先占位拿到 id，等子项/兄弟项建好后回写完整字典
        let dest =
            |page: lopdf::ObjectId| Object::Array(vec![page.into(), Object::Name(b"Fit".to_vec())]);
        let item0 = doc.new_object_id();
        let item1 = doc.add_object(Dict::from_iter([
            ("Title", utf16be("第一章")),
            ("Parent", item0.into()),
            ("Dest", dest(page_ids[1])),
        ]));
        let item2 = doc.add_object(Dict::from_iter([
            ("Title", utf16be("第二章")),
            ("Parent", outlines_id.into()),
            ("Dest", dest(page_ids[2])),
            ("Prev", item0.into()),
        ]));
        doc.set_object(
            item0,
            Dict::from_iter([
                (
                    "Title",
                    Object::String(b"Chapter A".to_vec(), StringFormat::Literal),
                ),
                ("Parent", outlines_id.into()),
                ("Dest", dest(page_ids[0])),
                ("Next", item2.into()),
                ("First", item1.into()),
            ]),
        );
        doc.set_object(
            outlines_id,
            Dict::from_iter([
                ("Type", Object::Name(b"Outlines".to_vec())),
                ("First", item0.into()),
                ("Last", item2.into()),
                ("Count", Object::Integer(3)),
            ]),
        );

        let catalog = Dict::from_iter([
            ("Type", Object::Name(b"Catalog".to_vec())),
            ("Pages", pages_id.into()),
            ("Outlines", outlines_id.into()),
        ]);
        let catalog_id = doc.add_object(catalog);
        let info_id = doc.add_object(Dict::from_iter([
            ("Title", utf16be("测试之书")),
            (
                "Author",
                Object::String(b"Author".to_vec(), StringFormat::Literal),
            ),
        ]));
        doc.trailer.set("Root", Object::Reference(catalog_id));
        doc.trailer.set("Info", Object::Reference(info_id));

        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn parse_pdf_metadata_and_outline() {
        let data = build_pdf();
        let parsed = parse_pdf(&data).unwrap();
        assert_eq!(parsed.num_pages, 3);
        assert_eq!(parsed.title.as_deref(), Some("测试之书"));
        assert_eq!(parsed.author.as_deref(), Some("Author"));
        // 先深后广：第一章的子项排在第一章与第二章之间
        assert_eq!(parsed.chapters, vec![
            ("Chapter A".to_string(), 0),
            ("第一章".to_string(), 1),
            ("第二章".to_string(), 2),
        ]);
    }

    #[test]
    fn parse_pdf_rejects_garbage() {
        assert!(matches!(
            parse_pdf(b"not a pdf"),
            Err(AppError::FileParseError(_))
        ));
    }
}
