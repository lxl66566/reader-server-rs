//! 服务端预生成 epub.js 兼容的 locations 数组
//!
//! 前端打开书后 `book.locations.generate(1600)` 需逐章拉取 xhtml（一章一
//! 请求 + 100ms 停顿）。本模块在服务端一次性复刻 epub.js 0.3.93 的分块与
//! CFI 编码规则生成全书数组，前端经 `book.locations.load()` 直接注入。
//!
//! 复刻的规则均以 epubjs 源码为准（locations.js parse / epubcfi.js
//! joinSteps / packaging.js parseSpine），细节见各函数注释。与浏览器解析
//! 的已知差异：roxmltree 把 CDATA 并入普通 Text 节点（DOM 中 CDATASection
//! 不占文本步进序号、不参与分块），XHTML 章节内 CDATA 极罕见，接受该偏差。

use std::{
    collections::HashMap,
    fmt::Write as _,
    io::{Read, Seek},
};

/// XML 解析统一入口：允许 DTD（实体注入兜底依赖内部子集的 ENTITY 声明）
fn parse_xml(text: &str) -> Result<roxmltree::Document<'_>, roxmltree::Error> {
    roxmltree::Document::parse_with_options(text, roxmltree::ParsingOptions {
        allow_dtd: true,
        ..Default::default()
    })
}

use crate::{
    error::AppError,
    parser::epub::{MAX_EPUB_ENTRY_BYTES, expand_self_closed_raw_text, normalize_zip_path},
};

/// 分块粒度（UTF-16 code units），与前端 `locations.generate(1600)` 一致
const GRANULARITY: u64 = 1600;

/// CFI 步进：元素（可带 id 断言）或文本节点
#[derive(Clone, PartialEq, Eq)]
enum Step {
    /// 元素在父的元素子节点中的 0-based 序号；id 为无命名空间的 id 属性
    /// （DOM `node.id` 的读取范围，xml:id 不算）
    Element { index: usize, id: Option<String> },
    /// 文本节点在父的文本子节点中的 0-based 序号（含纯空白文本节点）
    Text { index: usize },
}

/// 文档序收集的文本节点信息
struct TextNode {
    /// 从 body 到该节点的步进链（body → … → 父元素 → 文本节点）
    steps: Vec<Step>,
    /// `node.length`：UTF-16 code units 数（DOM CharacterData 的长度语义）
    utf16_len: u64,
    /// `textContent.trim().length === 0`：纯空白，分块时整节点跳过
    all_whitespace: bool,
}

/// epub.js `joinSteps`：元素步 `(index+1)*2`（可带 `[id]`），文本步 `1+2*index`
fn join_steps(steps: &[Step]) -> String {
    let mut out = String::new();
    for (i, step) in steps.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        match step {
            Step::Element { index, id } => {
                out.push_str(&((index + 1) * 2).to_string());
                if let Some(id) = id {
                    out.push('[');
                    out.push_str(id);
                    out.push(']');
                }
            },
            Step::Text { index } => out.push_str(&(1 + 2 * index).to_string()),
        }
    }
    out
}

/// epub.js `fromRange` 的共享路径提取：start/end 步进链逐位（index/id/类型）
/// 相等的前缀进入公共 path，但最多取 `start.len()-1` 步（源码对最后一步
/// 仅在两段 terminal 为同一对象时才并入，而两次 pathTo 的 terminal 永不相等）
fn shared_prefix_len(start: &[Step], end: &[Step]) -> usize {
    let last = start.len().saturating_sub(1);
    let mut shared = 0;
    for (i, (s, e)) in start.iter().zip(end).enumerate() {
        if s != e {
            break;
        }
        if i != last {
            shared = i + 1;
        }
    }
    shared
}

/// 生成一段 range CFI：`epubcfi(base!/path,start:offset,end:offset)`
/// （每个 segment 自带前导 `/`，同 epub.js `segmentString`）
fn range_cfi(base: &str, start: (&TextNode, u64), end: (&TextNode, u64)) -> String {
    let s = &start.0.steps;
    let e = &end.0.steps;
    let shared = shared_prefix_len(s, e);
    format!(
        "epubcfi({base}!/{path},/{start_steps}:{start_off},/{end_steps}:{end_off})",
        path = join_steps(&s[..shared]),
        start_steps = join_steps(&s[shared..]),
        start_off = start.1,
        end_steps = join_steps(&e[shared..]),
        end_off = end.1,
    )
}

/// JS `String.prototype.trim` 的空白集合（ECMAScript WhiteSpace ∪
/// LineTerminator）：与 Rust `char::is_whitespace` 差异在于多 `\u{FEFF}`、
/// 少 `\u{85}`
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// 深度优先找第一个指定名称的元素（epub.js `qs` 的语义）
fn find_first<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    local: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    if node.is_element() {
        if node.tag_name().name() == local {
            return Some(node);
        }
        for child in node.children() {
            if let Some(found) = find_first(child, local) {
                return Some(found);
            }
        }
    }
    None
}

/// 深度优先收集所有指定名称的元素（文档序，epub.js `qsa` 的语义）
fn find_all<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    local: &str,
) -> Vec<roxmltree::Node<'a, 'input>> {
    fn walk<'a, 'input>(
        node: roxmltree::Node<'a, 'input>,
        local: &str,
        out: &mut Vec<roxmltree::Node<'a, 'input>>,
    ) {
        if !node.is_element() {
            return;
        }
        if node.tag_name().name() == local {
            out.push(node);
        }
        for child in node.children() {
            walk(child, local, out);
        }
    }
    let mut out = Vec::new();
    walk(node, local, &mut out);
    out
}

/// spine itemref：`id` 是 itemref 元素自身的 id 属性（CFI 断言用它，不是
/// idref）；linear 仅在属性缺省或恰好为 "yes" 时为真（epub.js
/// `item.linear === "yes"` 的严格比较）
struct SpineItemRef {
    id: Option<String>,
    idref: String,
    linear: bool,
}

/// 解析 OPF：返回 (spineNodeIndex, spine 列表, manifest id→href)
/// OPF 解析结果：(spineNodeIndex, spine itemref 列表, manifest id→href)
type OpfInfo = (usize, Vec<SpineItemRef>, HashMap<String, String>);

fn parse_opf(doc: &roxmltree::Document) -> Result<OpfInfo, AppError> {
    let root = doc.root_element();
    let spine = find_first(root, "spine")
        .ok_or_else(|| AppError::FileParseError("EPUB OPF has no spine".into()))?;

    // spineNodeIndex：spine 在其父元素的元素子节点中的 0-based 序号
    // （epub.js indexOfElementNode；标准 OPF 为 metadata/manifest/spine → 2）
    let spine_node_index = spine.parent().map_or(0, |parent| {
        parent
            .children()
            .filter(roxmltree::Node::is_element)
            .take_while(|c| *c != spine)
            .count()
    });

    let mut itemrefs = Vec::new();
    for item in find_all(spine, "itemref") {
        let Some(idref) = item.attribute("idref") else {
            continue;
        };
        itemrefs.push(SpineItemRef {
            id: item.attribute("id").map(str::to_string),
            idref: idref.to_string(),
            linear: item.attribute("linear").unwrap_or("yes") == "yes",
        });
    }

    let mut manifest = HashMap::new();
    if let Some(manifest_el) = find_first(root, "manifest") {
        for item in find_all(manifest_el, "item") {
            if let (Some(id), Some(href)) = (item.attribute("id"), item.attribute("href")) {
                manifest.insert(id.to_string(), href.to_string());
            }
        }
    }

    Ok((spine_node_index, itemrefs, manifest))
}

/// 收集 body 子树内的全部文本节点（文档序），构建各自到 body 的步进链
fn collect_text_nodes(body: roxmltree::Node) -> Vec<TextNode> {
    fn walk(node: roxmltree::Node, steps: &mut Vec<Step>, out: &mut Vec<TextNode>) {
        let mut elem_idx = 0usize;
        let mut text_idx = 0usize;
        for child in node.children() {
            if child.is_text() {
                let text = child.text().unwrap_or_default();
                steps.push(Step::Text { index: text_idx });
                out.push(TextNode {
                    steps: steps.clone(),
                    utf16_len: text.encode_utf16().count() as u64,
                    all_whitespace: text.chars().all(is_js_whitespace),
                });
                steps.pop();
                text_idx += 1;
            } else if child.is_element() {
                steps.push(Step::Element {
                    index: elem_idx,
                    id: child.attribute("id").map(str::to_string),
                });
                walk(child, steps, out);
                steps.pop();
                elem_idx += 1;
            }
        }
    }
    let mut out = Vec::new();
    walk(body, &mut Vec::new(), &mut out);
    out
}

/// epub.js `Locations.parse` 状态机的精确复刻
///
/// - 纯空白节点整节点跳过（不更新 prev）
/// - counter==0 时 while 外开 range（offset 0）、while 内 `pos+=1` 后重开 range（源码的 off-by-one
///   原样保留）
/// - 跨节点累计 counter，凑满一个 chunk 即产出一条 range CFI 并清零
/// - 章末用最后一个非空白文本节点收尾
fn chapter_locations(body: roxmltree::Node, base: &str) -> Vec<String> {
    // body 自身也占一步（epub.js pathTo 向上走到 html 为止，body 的步进计入
    // 路径；标准 XHTML 中 head/body 依次为 html 的第 1/2 个元素子节点）
    let body_step = Step::Element {
        index: body.parent().map_or(0, |p| {
            p.children()
                .filter(roxmltree::Node::is_element)
                .take_while(|c| *c != body)
                .count()
        }),
        id: body.attribute("id").map(str::to_string),
    };
    let mut nodes = collect_text_nodes(body);
    for node in &mut nodes {
        node.steps.insert(0, body_step.clone());
    }
    let mut locations = Vec::new();
    let mut counter: u64 = 0;
    let mut prev: Option<usize> = None;
    // 当前未闭合 range 的起点 (节点下标, offset)
    let mut range_start: Option<(usize, u64)> = None;

    for (idx, node) in nodes.iter().enumerate() {
        let len = node.utf16_len;
        if node.all_whitespace {
            continue;
        }
        if counter == 0 {
            range_start = Some((idx, 0));
        }
        let mut pos: u64 = 0;
        if GRANULARITY - counter > len {
            counter += len;
            pos = len;
        }
        while pos < len {
            let dist = GRANULARITY - counter;
            if counter == 0 {
                pos += 1;
                range_start = Some((idx, pos));
            }
            if pos + dist >= len {
                counter += len - pos;
                pos = len;
            } else {
                pos += dist;
                let start = range_start.expect("range start must be set when counter is 0");
                locations.push(range_cfi(base, (&nodes[start.0], start.1), (node, pos)));
                counter = 0;
            }
        }
        prev = Some(idx);
    }

    if let (Some(start), Some(prev_idx)) = (range_start, prev) {
        let end_node = &nodes[prev_idx];
        locations.push(range_cfi(
            base,
            (&nodes[start.0], start.1),
            (end_node, end_node.utf16_len),
        ));
    }
    locations
}

/// XHTML 常见命名实体（EPUB2 的 DOCTYPE 引用外部 DTD，浏览器 XML 解析器
/// 内置 HTML 实体表而 roxmltree 只认 XML 预定义实体）。解析失败时把这张表
/// 注入 DOCTYPE 内部子集重试，覆盖绝大多数 EPUB2 转制书；已有内部子集的
/// 文档跳过注入
const HTML_ENTITIES: &[(&str, u32)] = &[
    ("nbsp", 160),
    ("iexcl", 161),
    ("cent", 162),
    ("pound", 163),
    ("curren", 164),
    ("yen", 165),
    ("brvbar", 166),
    ("sect", 167),
    ("uml", 168),
    ("copy", 169),
    ("ordf", 170),
    ("laquo", 171),
    ("not", 172),
    ("shy", 173),
    ("reg", 174),
    ("macr", 175),
    ("deg", 176),
    ("plusmn", 177),
    ("sup2", 178),
    ("sup3", 179),
    ("acute", 180),
    ("micro", 181),
    ("para", 182),
    ("middot", 183),
    ("cedil", 184),
    ("sup1", 185),
    ("ordm", 186),
    ("raquo", 187),
    ("frac14", 188),
    ("frac12", 189),
    ("frac34", 190),
    ("iquest", 191),
    ("Agrave", 192),
    ("Aacute", 193),
    ("Acirc", 194),
    ("Atilde", 195),
    ("Auml", 196),
    ("Aring", 197),
    ("AElig", 198),
    ("Ccedil", 199),
    ("Egrave", 200),
    ("Eacute", 201),
    ("Ecirc", 202),
    ("Euml", 203),
    ("Igrave", 204),
    ("Iacute", 205),
    ("Icirc", 206),
    ("Iuml", 207),
    ("ETH", 208),
    ("Ntilde", 209),
    ("Ograve", 210),
    ("Oacute", 211),
    ("Ocirc", 212),
    ("Otilde", 213),
    ("Ouml", 214),
    ("times", 215),
    ("Oslash", 216),
    ("Ugrave", 217),
    ("Uacute", 218),
    ("Ucirc", 219),
    ("Uuml", 220),
    ("Yacute", 221),
    ("THORN", 222),
    ("szlig", 223),
    ("agrave", 224),
    ("aacute", 225),
    ("acirc", 226),
    ("atilde", 227),
    ("auml", 228),
    ("aring", 229),
    ("aelig", 230),
    ("ccedil", 231),
    ("egrave", 232),
    ("eacute", 233),
    ("ecirc", 234),
    ("euml", 235),
    ("igrave", 236),
    ("iacute", 237),
    ("icirc", 238),
    ("iuml", 239),
    ("eth", 240),
    ("ntilde", 241),
    ("ograve", 242),
    ("oacute", 243),
    ("ocirc", 244),
    ("otilde", 245),
    ("ouml", 246),
    ("divide", 247),
    ("oslash", 248),
    ("ugrave", 249),
    ("uacute", 250),
    ("ucirc", 251),
    ("uuml", 252),
    ("yacute", 253),
    ("thorn", 254),
    ("yuml", 255),
    ("OElig", 338),
    ("oelig", 339),
    ("Scaron", 352),
    ("scaron", 353),
    ("Yuml", 376),
    ("fnof", 402),
    ("circ", 710),
    ("tilde", 732),
    ("Alpha", 913),
    ("Beta", 914),
    ("Gamma", 915),
    ("Delta", 916),
    ("Epsilon", 917),
    ("Zeta", 918),
    ("Eta", 919),
    ("Theta", 920),
    ("Iota", 921),
    ("Kappa", 922),
    ("Lambda", 923),
    ("Mu", 924),
    ("Nu", 925),
    ("Xi", 926),
    ("Omicron", 927),
    ("Pi", 928),
    ("Rho", 929),
    ("Sigma", 931),
    ("Tau", 932),
    ("Upsilon", 933),
    ("Phi", 934),
    ("Chi", 935),
    ("Psi", 936),
    ("Omega", 937),
    ("alpha", 945),
    ("beta", 946),
    ("gamma", 947),
    ("delta", 948),
    ("epsilon", 949),
    ("zeta", 950),
    ("eta", 951),
    ("theta", 952),
    ("iota", 953),
    ("kappa", 954),
    ("lambda", 955),
    ("mu", 956),
    ("nu", 957),
    ("xi", 958),
    ("omicron", 959),
    ("pi", 960),
    ("rho", 961),
    ("sigmaf", 962),
    ("sigma", 963),
    ("tau", 964),
    ("upsilon", 965),
    ("phi", 966),
    ("chi", 967),
    ("psi", 968),
    ("omega", 969),
    ("thetasym", 977),
    ("upsih", 978),
    ("piv", 982),
    ("ensp", 8194),
    ("emsp", 8195),
    ("thinsp", 8201),
    ("zwnj", 8204),
    ("zwj", 8205),
    ("lrm", 8206),
    ("rlm", 8207),
    ("ndash", 8211),
    ("mdash", 8212),
    ("lsquo", 8216),
    ("rsquo", 8217),
    ("sbquo", 8218),
    ("ldquo", 8220),
    ("rdquo", 8221),
    ("bdquo", 8222),
    ("dagger", 8224),
    ("Dagger", 8225),
    ("bull", 8226),
    ("hellip", 8230),
    ("permil", 8240),
    ("prime", 8242),
    ("Prime", 8243),
    ("lsaquo", 8249),
    ("rsaquo", 8250),
    ("oline", 8254),
    ("frasl", 8260),
    ("euro", 8364),
    ("image", 8465),
    ("weierp", 8472),
    ("real", 8476),
    ("trade", 8482),
    ("alefsym", 8501),
    ("larr", 8592),
    ("uarr", 8593),
    ("rarr", 8594),
    ("darr", 8595),
    ("harr", 8596),
    ("crarr", 8629),
    ("lArr", 8656),
    ("uArr", 8657),
    ("rArr", 8658),
    ("dArr", 8659),
    ("hArr", 8660),
    ("forall", 8704),
    ("part", 8706),
    ("exist", 8707),
    ("empty", 8709),
    ("nabla", 8711),
    ("isin", 8712),
    ("notin", 8713),
    ("ni", 8715),
    ("prod", 8719),
    ("sum", 8721),
    ("minus", 8722),
    ("lowast", 8727),
    ("radic", 8730),
    ("prop", 8733),
    ("infin", 8734),
    ("ang", 8736),
    ("and", 8743),
    ("or", 8744),
    ("cap", 8745),
    ("cup", 8746),
    ("int", 8747),
    ("there4", 8756),
    ("sim", 8764),
    ("cong", 8773),
    ("asymp", 8776),
    ("ne", 8800),
    ("equiv", 8801),
    ("le", 8804),
    ("ge", 8805),
    ("sub", 8834),
    ("sup", 8835),
    ("nsub", 8836),
    ("sube", 8838),
    ("supe", 8839),
    ("oplus", 8853),
    ("otimes", 8855),
    ("perp", 8869),
    ("sdot", 8901),
    ("lceil", 8968),
    ("rceil", 8969),
    ("lfloor", 8970),
    ("rfloor", 8971),
    ("lang", 9001),
    ("rang", 9002),
    ("loz", 9674),
    ("spades", 9824),
    ("clubs", 9827),
    ("hearts", 9829),
    ("diams", 9830),
];

/// DOCTYPE 未定义实体兜底：把 HTML 实体声明注入返回新文本。
/// - 有 DOCTYPE：插入其内部子集（已含 `[` 子集的放弃）
/// - 无 DOCTYPE：跳过 XML 声明后前置一个带实体子集的 DOCTYPE （浏览器 XML 解析器内置 HTML
///   实体表，无 DOCTYPE 也认命名实体）
fn inject_html_entities(text: &str) -> Option<String> {
    let mut subset = String::from(" [");
    for (name, code) in HTML_ENTITIES {
        let _ = write!(subset, "<!ENTITY {name} \"&#{code};\">");
    }
    subset.push(']');

    if let Some(doctype_pos) = text.find("<!DOCTYPE") {
        let gt = doctype_pos + text[doctype_pos..].find('>')?;
        if text[doctype_pos..gt].contains('[') {
            return None;
        }
        let mut rewritten = String::with_capacity(text.len() + subset.len());
        rewritten.push_str(&text[..gt]);
        rewritten.push_str(&subset);
        rewritten.push_str(&text[gt..]);
        Some(rewritten)
    } else {
        let insert_at = match text.find("?>") {
            Some(gt) if text.starts_with("<?xml") => gt + 2,
            _ => 0,
        };
        let mut rewritten = String::with_capacity(text.len() + subset.len() + 16);
        rewritten.push_str(&text[..insert_at]);
        rewritten.push_str("<!DOCTYPE html");
        rewritten.push_str(&subset);
        rewritten.push('>');
        rewritten.push_str(&text[insert_at..]);
        Some(rewritten)
    }
}

/// 解析章节 XML 并生成 locations；解析失败时注入 HTML 实体重试一次，
/// 仍失败（或无 body）返回 None。Document 生命周期封闭在函数内
fn parse_chapter_locations(text: &str, base: &str) -> Option<Vec<String>> {
    let attempt = |t: &str| -> Option<Vec<String>> {
        let doc = parse_xml(t).ok()?;
        let body = find_first(doc.root_element(), "body")?;
        Some(chapter_locations(body, base))
    };
    attempt(text).or_else(|| inject_html_entities(text).and_then(|rw| attempt(&rw)))
}

/// 解析 OPF；解析失败时注入 HTML 实体重试一次，仍失败按 FileParseError 上报
fn parse_opf_document(text: &str) -> Result<OpfInfo, AppError> {
    let attempt = |t: &str| -> Option<_> {
        let doc = parse_xml(t).ok()?;
        parse_opf(&doc).ok()
    };
    attempt(text)
        .or_else(|| inject_html_entities(text).and_then(|rw| attempt(&rw)))
        .ok_or_else(|| AppError::FileParseError("invalid EPUB OPF".into()))
}

/// 解析失败章节的兜底锚点：body 在 html 下的常规位置（第 2 个元素子节点，
/// 步进 4）。保证数组按章序非空且可寻址，粒度退化为每章一点
fn fallback_anchor(base: &str) -> String {
    format!("epubcfi({base}!/4)")
}

/// 从 zip 读文本条目（限解压大小上限）；条目缺失或非 UTF-8 返回 None
fn read_entry_to_str<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    entry: &str,
) -> Result<Option<String>, AppError> {
    let Ok(zip_entry) = archive.by_name(entry) else {
        return Ok(None);
    };
    let mut buf = Vec::with_capacity(
        usize::try_from(zip_entry.size().min(MAX_EPUB_ENTRY_BYTES))
            .expect("entry size must fit in usize"),
    );
    let mut limited = zip_entry.take(MAX_EPUB_ENTRY_BYTES + 1);
    limited
        .read_to_end(&mut buf)
        .map_err(|e| AppError::FileParseError(format!("corrupted EPUB entry: {e}")))?;
    if buf.len() as u64 > MAX_EPUB_ENTRY_BYTES {
        return Err(AppError::EpubContentTooLarge);
    }
    match String::from_utf8(buf) {
        Ok(text) => Ok(Some(text)),
        Err(_) => Ok(None),
    }
}

/// href 相对 OPF 目录解析为 zip 条目路径：去 fragment、percent-decode、
/// 归一化（浏览器 URL resolve 会解码百分号编码，zip 条目名按解码后匹配）
fn resolve_zip_path(opf_dir: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);
    let decoded = percent_encoding::percent_decode_str(href).decode_utf8_lossy();
    let joined = if opf_dir.is_empty() {
        decoded.into_owned()
    } else {
        format!("{opf_dir}/{decoded}")
    };
    normalize_zip_path(&joined)
}

/// 从 EPUB 字节数据生成全书 locations（granularity 1600），数组按 spine
/// 文档序严格递增，可直接注入 `book.locations.load()`
pub fn generate_locations(zip_data: &[u8]) -> Result<Vec<String>, AppError> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_data))
        .map_err(|e| AppError::FileParseError(format!("invalid EPUB zip: {e}")))?;

    let container = read_entry_to_str(&mut archive, "META-INF/container.xml")?
        .ok_or_else(|| AppError::FileParseError("EPUB missing META-INF/container.xml".into()))?;
    let container_doc = roxmltree::Document::parse(&container)
        .map_err(|e| AppError::FileParseError(format!("invalid container.xml: {e}")))?;
    let opf_path = find_first(container_doc.root_element(), "rootfile")
        .and_then(|rf| rf.attribute("full-path"))
        .ok_or_else(|| AppError::FileParseError("container.xml has no rootfile".into()))?
        .to_string();

    let opf = read_entry_to_str(&mut archive, &opf_path)?
        .ok_or_else(|| AppError::FileParseError("EPUB OPF entry missing".into()))?;
    let (spine_node_index, itemrefs, manifest) = parse_opf_document(&opf)?;

    let opf_dir = opf_path.rsplit_once('/').map_or("", |(dir, _)| dir);

    let mut locations = Vec::new();
    for (spine_pos, item) in itemrefs.iter().enumerate() {
        // spinePos 含 linear=no 的章节（CFI 步进按位计数），但不为其生成分块
        if !item.linear {
            continue;
        }
        let Some(href) = manifest.get(&item.idref) else {
            continue;
        };
        // 章 base：spine 步进 + itemref 自身 id 断言（空串 id 视为无断言）
        let id_assert = item
            .id
            .as_deref()
            .filter(|id| !id.is_empty())
            .map(|id| format!("[{id}]"))
            .unwrap_or_default();
        let base = format!(
            "/{}/{}{id_assert}",
            (spine_node_index + 1) * 2,
            (spine_pos + 1) * 2,
        );

        let entry = resolve_zip_path(opf_dir, href);
        let Some(chapter) = read_entry_to_str(&mut archive, &entry)? else {
            locations.push(fallback_anchor(&base));
            continue;
        };
        // 与懒加载端点同源清洗后按 XML 语义解析，保证与浏览器树一致
        let cleaned = expand_self_closed_raw_text(&chapter);
        let chunks = parse_chapter_locations(&cleaned, &base)
            .unwrap_or_else(|| vec![fallback_anchor(&base)]);
        locations.extend(chunks);
    }

    Ok(locations)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造含多个章节文件的最小 EPUB
    fn build_epub(opf: &str, chapters: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write as _;

        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            let deflated = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
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
            writer.write_all(opf.as_bytes()).unwrap();
            for (name, html) in chapters {
                writer.start_file(*name, deflated).unwrap();
                writer.write_all(html.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn opf(itemrefs: &str, extra_items: &str) -> String {
        format!(
            r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>T</dc:title></metadata>
  <manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/><item id="c3" href="c3.xhtml" media-type="application/xhtml+xml"/>{extra_items}</manifest>
  <spine>{itemrefs}</spine>
</package>"#,
        )
    }

    /// body(p1) → 文本步进 [4(body), 2(p), 1(text)]；body 下 p 是首个元素
    fn xhtml(body: &str) -> String {
        format!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head><body>{body}</body></html>"#,
        )
    }

    #[test]
    fn single_node_chunking_and_offsets() {
        // 4000 字符单文本节点：3 段 range，start 偏移含源码 off-by-one(+1)
        let text = "A".repeat(4000);
        let data = build_epub(&opf(r#"<itemref idref="c1"/>"#, ""), &[(
            "OEBPS/c1.xhtml",
            &xhtml(&format!("<p>{text}</p>")),
        )]);
        let locs = generate_locations(&data).unwrap();
        assert_eq!(locs, vec![
            "epubcfi(/6/2!/4/2,/1:1,/1:1601)",
            "epubcfi(/6/2!/4/2,/1:1602,/1:3202)",
            "epubcfi(/6/2!/4/2,/1:3203,/1:4000)",
        ],);
    }

    #[test]
    fn cross_node_accumulation_and_id_assertion() {
        // 1550 + 50：凑满一个 chunk；itemref id 断言进 base；第二个 p/text 步进按位递增
        let t1 = "B".repeat(1550);
        let t2 = "C".repeat(50);
        let data = build_epub(
            &opf(
                r#"<itemref idref="c1"/><itemref id="sec2" idref="c2"/>"#,
                "",
            ),
            &[
                ("OEBPS/c1.xhtml", &xhtml(&format!("<p>{t1}</p>"))),
                ("OEBPS/c2.xhtml", &xhtml(&format!("<p>{t1}</p><p>{t2}</p>"))),
            ],
        );
        let locs = generate_locations(&data).unwrap();
        assert_eq!(locs, vec![
            // 第一章：1550 < 1600，章末收尾一条
            "epubcfi(/6/2!/4/2,/1:0,/1:1550)",
            // 第二章：跨节点凑满 1600，起点是首节点 offset 0
            "epubcfi(/6/4[sec2]!/4,/2/1:0,/4/1:50)",
        ],);
    }

    #[test]
    fn whitespace_nodes_skipped_without_updating_prev() {
        // 中间的纯空白 p 不参与累计也不更新 prev；末尾空白 p 同样被忽略
        let t1 = "D".repeat(1550);
        let t3 = "E".repeat(50);
        let data = build_epub(&opf(r#"<itemref idref="c1"/>"#, ""), &[(
            "OEBPS/c1.xhtml",
            &xhtml(&format!("<p>{t1}</p><p>   </p><p>{t3}</p><p>\n\t</p>")),
        )]);
        let locs = generate_locations(&data).unwrap();
        // 四个 p 依次占元素步进 2/4/6/8；起止分属第 1、3 个 p（各自的首个
        // 文本子节点），公共路径只到 body（步进 4）
        assert_eq!(locs, vec!["epubcfi(/6/2!/4,/2/1:0,/6/1:50)"]);
    }

    #[test]
    fn non_linear_and_unknown_idref_keep_spine_positions() {
        // linear=no 章节占 spinePos 但不产条目；idref 缺 manifest 的章节跳过
        let t = "F".repeat(100);
        let data = build_epub(
            &opf(
                r#"<itemref idref="nope"/><itemref idref="c1" linear="no"/><itemref idref="c2"/>"#,
                "",
            ),
            &[
                ("OEBPS/c1.xhtml", &xhtml(&format!("<p>{t}</p>"))),
                ("OEBPS/c2.xhtml", &xhtml(&format!("<p>{t}</p>"))),
            ],
        );
        let locs = generate_locations(&data).unwrap();
        // 仅 c2 产出，且其 spinePos=2（步进 6）
        assert_eq!(locs, vec!["epubcfi(/6/6!/4/2,/1:0,/1:100)"]);
    }

    #[test]
    fn broken_chapter_falls_back_to_chapter_anchor() {
        // 非 XML 内容（未闭合标签）→ 解析失败 → 章首锚点兜底
        let data = build_epub(&opf(r#"<itemref idref="c1"/>"#, ""), &[(
            "OEBPS/c1.xhtml",
            "<html><body><p>unclosed",
        )]);
        let locs = generate_locations(&data).unwrap();
        assert_eq!(locs, vec!["epubcfi(/6/2!/4)"]);
    }

    #[test]
    fn missing_chapter_entry_falls_back() {
        // spine 指向的条目不在 zip 内 → 章首锚点兜底
        let data = build_epub(&opf(r#"<itemref idref="c3"/>"#, ""), &[(
            "OEBPS/c1.xhtml",
            &xhtml("<p>x</p>"),
        )]);
        let locs = generate_locations(&data).unwrap();
        assert_eq!(locs, vec!["epubcfi(/6/2!/4)"]);
    }

    #[test]
    fn astral_chars_counted_as_utf16_units() {
        // DOM node.length 按 UTF-16 code units 计：emoji 占 2；1599 个 BMP 字符
        // + 一个 2 单位 emoji = 1601 单位 > 1600，产出两段
        let mut text = "G".repeat(1599);
        text.push('😀');
        assert_eq!(text.encode_utf16().count(), 1601);
        let data = build_epub(&opf(r#"<itemref idref="c1"/>"#, ""), &[(
            "OEBPS/c1.xhtml",
            &xhtml(&format!("<p>{text}</p>")),
        )]);
        let locs = generate_locations(&data).unwrap();
        // counter 累计到 1599 后剩余 dist=1：pos=1 起步，1+1599>=1599 →
        // 第一轮累计(counter=1599)，无 push；第二轮 pos 已 len 退出循环，
        // 章末收尾一条
        assert_eq!(locs, vec!["epubcfi(/6/2!/4/2,/1:1,/1:1601)"]);
    }

    #[test]
    fn named_entities_resolved_via_doctype_injection() {
        // &nbsp; 等命名实体：浏览器 XML 解析器内置支持；注入内部子集后对齐
        let body = format!("<p>{}&nbsp;end</p>", "H".repeat(1600));
        let data = build_epub(&opf(r#"<itemref idref="c1"/>"#, ""), &[(
            "OEBPS/c1.xhtml",
            &xhtml(&body),
        )]);
        let locs = generate_locations(&data).unwrap();
        // 1600 + 1(nbsp) + 3 = 1604 单位：第一段 [1,1601]，章末收尾 [1602,1604]
        assert_eq!(locs, vec![
            "epubcfi(/6/2!/4/2,/1:1,/1:1601)",
            "epubcfi(/6/2!/4/2,/1:1602,/1:1604)",
        ],);
    }

    #[test]
    fn href_with_percent_encoding_and_fragment() {
        // href 带百分号编码（c%31 → c1）与 fragment：按解码后的 zip 条目名解析
        let opf_encoded = opf(r#"<itemref idref="c1"/>"#, "")
            .replace(r#"href="c1.xhtml""#, r#"href="c%31.xhtml#frag""#);
        let data = build_epub(&opf_encoded, &[("OEBPS/c1.xhtml", &xhtml("<p>plain</p>"))]);
        let locs = generate_locations(&data).unwrap();
        assert_eq!(locs, vec!["epubcfi(/6/2!/4/2,/1:0,/1:5)"]);
    }
}

#[cfg(test)]
mod sanity_tests {
    use super::*;

    // 真实 e2e 测试书（40 章、每章 < 一个分块粒度）的结构性检查
    #[test]
    fn sample_epub_sanity() {
        let data = std::fs::read("frontend/e2e/assets/sample.epub").unwrap();
        let locs = generate_locations(&data).unwrap();
        assert_eq!(locs.len(), 40, "one location per short chapter");
        assert!(
            locs.iter().all(|c| c.starts_with("epubcfi(/6/")),
            "standard OPF spine index"
        );
        assert_eq!(
            locs.iter().filter(|c| c.ends_with("!/4)")).count(),
            0,
            "no fallback anchor"
        );
        // spinePos 步进随章序严格递增：/6/2 → /6/4 → … → /6/80
        let spine_steps: Vec<u32> = locs
            .iter()
            .map(|c| {
                c.strip_prefix("epubcfi(/6/")
                    .unwrap()
                    .split(['/', '!'])
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            })
            .collect();
        assert!(spine_steps.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(spine_steps.first(), Some(&2));
        assert_eq!(spine_steps.last(), Some(&80));
    }
}
