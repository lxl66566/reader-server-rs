// 段首缩进兼容：中文书籍常以全角空格（U+3000）等字符做段首缩进，而阅读器
// 用 CSS text-indent 接管缩进，两者叠加会加倍（2 空格 + 2em = 4 字）。
// 凡阅读器强制施加 text-indent 的渲染路径，都应剥离原文字符缩进。

/** 剥离 epub 文档中 p 段首的空白文本（主题对 p 强制 text-indent，含嵌套 span） */
export function stripEpubParagraphIndent(doc: Document): void {
  for (const p of doc.querySelectorAll('p')) {
    const walker = doc.createTreeWalker(p, NodeFilter.SHOW_TEXT);
    // 逐个处理首个非空白字符之前的文本节点，段内空白不动
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const text = node.textContent ?? '';
      const stripped = text.replace(/^\s+/u, '');
      if (stripped !== text) node.textContent = stripped;
      if (stripped !== '') break;
    }
  }
}

/** 剥离段落行首空白（\s 已含全角空格 U+3000） */
export function stripParagraphIndent(text: string): string {
  return text.replace(/^\s+/u, '');
}
