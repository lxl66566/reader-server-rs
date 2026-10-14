// 生成 e2e 采样书籍：node e2e/assets/gen-assets.mjs
// - novel.txt：多章 UTF-8 文本，第一章长章覆盖深链 locator=8000 的场景
// - sample.epub：EPUB3（nav + spine + 40 章 xhtml）
// 正文为确定性合成文本：行首不含章节标题词、行长 > 30，不命中 txt 切章规则。
import { writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import JSZip from 'jszip';

const outDir = dirname(fileURLToPath(import.meta.url));

function buildNovel() {
  // 第一章约 12.7k 字：locator=8000 落在章内中段，章前章后都留有余量
  const chapters = [
    ['第一章 长街', 250],
    ['第二章 渡口', 170],
    ['第三章 归途', 170],
    ['尾声', 3],
  ];
  return chapters
    .map(([title, paras]) => `${title}\n${chapterBody(paras)}`)
    .join('\n');
}

function chapterBody(paras) {
  return Array.from({ length: paras }, (_, i) => paragraph(i)).join('\n');
}

/** 合成一段正文（> 30 字，行首无标题词） */
function paragraph(index) {
  return `这是编号${String(index).padStart(4, '0')}的合成正文段落，描述小镇上日常的细碎光景与人物往来，句子刻意写长以避免被识别为章节标题。`;
}

const EPUB_CHAPTERS = 40;

async function buildEpub() {
  const zip = new JSZip();
  // EPUB 规范：mimetype 必须是首个条目且不压缩
  zip.file('mimetype', 'application/epub+zip', { compression: 'STORE' });
  zip.file(
    'META-INF/container.xml',
    `<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>`,
  );

  const items = [];
  const refs = [];
  const toc = [];
  for (let i = 1; i <= EPUB_CHAPTERS; i++) {
    const id = `c${i}`;
    const href = `${id}.xhtml`;
    items.push(
      `<item id="${id}" href="${href}" media-type="application/xhtml+xml"/>`,
    );
    refs.push(`<itemref idref="${id}"/>`);
    toc.push(`<li><a href="${href}">第${i}章</a></li>`);
    const title = `第${i}章 样章`;
    const body = Array.from(
      { length: 14 },
      (_, p) => `<p>${paragraph(p)}</p>`,
    ).join('\n      ');
    zip.file(
      `OEBPS/${href}`,
      `<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <head><title>${title}</title></head>
  <body>
    <h1>${title}</h1>
    ${body}
  </body>
</html>`,
    );
  }

  zip.file(
    'OEBPS/nav.xhtml',
    `<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
  <head><title>目录</title></head>
  <body>
    <nav epub:type="toc"><h1>目录</h1>
      <ol>
        ${toc.join('\n        ')}
      </ol>
    </nav>
  </body>
</html>`,
  );
  zip.file(
    'OEBPS/content.opf',
    `<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">urn:uuid:e2e-sample-book</dc:identifier>
    <dc:title>合成样本之书</dc:title>
    <dc:creator>gen-assets</dc:creator>
    <dc:language>zh-CN</dc:language>
    <meta property="dcterms:modified">2026-10-15T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    ${items.join('\n    ')}
  </manifest>
  <spine>
    ${refs.join('\n    ')}
  </spine>
</package>`,
  );

  return zip.generateAsync({
    compression: 'DEFLATE',
    type: 'nodebuffer',
  });
}

const novel = buildNovel();
writeFileSync(join(outDir, 'novel.txt'), novel, 'utf8');
writeFileSync(join(outDir, 'sample.epub'), await buildEpub());
console.log(
  `novel.txt: ${novel.length} chars, sample.epub: ${EPUB_CHAPTERS} chapters`,
);
