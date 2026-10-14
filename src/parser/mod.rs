/// 书籍文件解析：格式检测与各格式（txt/epub/pdf）的元数据、章节提取
pub mod chapter;
pub mod epub;
pub mod format;
pub mod pdf;

pub use chapter::{extract_chapters, validate_chapter_pattern};
pub use epub::parse_epub;
pub use format::detect_format;
pub use pdf::parse_pdf;
