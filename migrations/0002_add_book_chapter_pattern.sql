-- 每本书独立的章节切分正则（仅 txt 有意义）；
-- NULL/空 = 使用内置默认章节规则（见 src/parser/chapter.rs 的 CHAPTER_RE）
ALTER TABLE books ADD COLUMN chapter_pattern TEXT;
