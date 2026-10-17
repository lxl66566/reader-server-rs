// 生成 e2e 采样书籍：node e2e/assets/gen-assets.mjs
// - novel.txt：多章 UTF-8 文本，第一章长章覆盖深链 locator=8000 的场景
// - sample.epub：EPUB3（nav + spine + 40 章 xhtml）
// - sample.pdf：12 页标准字体文本页 + Info（UTF-16BE 标题）+ 两层 outline 书签
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

// sample.pdf：无压缩手写 PDF（对象偏移在本函数内计算）。12 页 Helvetica
// 文本；Info.Title 为 UTF-16BE（中文 PDF 的普遍写法，回归后端解码）；
// outline 两层：第 0/5/11 页三个顶层条目，第 1 页一个子条目。
const PDF_PAGES = 12;

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
  <head><title>${title}</title>
  <link href="style.css" rel="stylesheet" type="text/css"/>
  </head>
  <body>
    <h1>${title}</h1>
    ${body}
  </body>
</html>`,
    );
  }

  // 书源强制样式：真实书籍常以 !important 锁死黑字白底（如
  // p { color:#000 !important }），阅读主题必须能压过它
  zip.file(
    'OEBPS/style.css',
    `body { color: #000 !important; background: #fff !important; }
p { color: #000 !important; }
h1 { color: #000 !important; background: #f5f5f5 !important; }
`,
  );

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

function buildPdf() {
  // Node 无 utf16be 编码：按 utf16le 输出后逐对换字节序
  const utf16beHex = text => {
    const le = Buffer.from(`\uFEFF${text}`, 'utf16le');
    for (let i = 0; i < le.length; i += 2) {
      [le[i], le[i + 1]] = [le[i + 1], le[i]];
    }
    return le.toString('hex').toUpperCase();
  };

  // (标题, 目标页 0-based)；子条目挂在「中段」下。非 ASCII 标题写为
  // UTF-16BE 十六进制串（中文 PDF 的普遍写法），ASCII 标题写为字面串
  const chapters = [
    { page: 0, parent: 'root', title: '开篇' },
    { page: 5, parent: 'root', title: '中段' },
    { page: 1, parent: '中段', title: '小节' },
    { page: 11, parent: 'root', title: 'Appendix' },
  ];

  const objects = [];
  const add = body => {
    objects.push(body);
    return objects.length; // 1-based obj number
  };

  const catalogId = add('');
  const pagesId = add('');
  const outlinesId = add('');
  const fontId = add(
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>',
  );
  const pageIds = [];
  const contentIds = [];
  for (let i = 0; i < PDF_PAGES; i++) {
    contentIds.push(
      add(
        `<< /Length ${`BT /F1 28 Tf 72 700 Td (Sample PDF page ${i + 1}) Tj ET`.length} >>\nstream\nBT /F1 28 Tf 72 700 Td (Sample PDF page ${i + 1}) Tj ET\nendstream`,
      ),
    );
    pageIds.push(
      add(
        `<< /Type /Page /Parent ${pagesId} 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 ${fontId} 0 R >> >> /Contents ${contentIds[i]} 0 R >>`,
      ),
    );
  }

  // outline 条目对象：先占位拿到编号再回填 First/Next/Prev 链
  const itemIds = chapters.map(() => add(''));
  const items = chapters.map((chapter, i) => {
    const siblings = chapters
      .map((c, j) => ({ c, j }))
      .filter(({ c }) => c.parent === chapter.parent);
    const pos = siblings.findIndex(({ j }) => j === i);
    const title = /^[\x20-\x7e]*$/.test(chapter.title)
      ? `(${chapter.title})`
      : `<${utf16beHex(chapter.title)}>`;
    const fields = [
      `/Title ${title}`,
      `/Parent ${chapter.parent === 'root' ? outlinesId : itemIds[chapters.findIndex(c => c.title === chapter.parent)]} 0 R`,
      `/Dest [${pageIds[chapter.page]} 0 R /Fit]`,
    ];
    if (pos > 0) fields.push(`/Prev ${itemIds[siblings[pos - 1].j]} 0 R`);
    if (pos < siblings.length - 1) {
      fields.push(`/Next ${itemIds[siblings[pos + 1].j]} 0 R`);
    }
    if (chapter.parent === 'root') {
      const children = chapters
        .map((c, j) => ({ c, j }))
        .filter(({ c }) => c.parent === chapter.title);
      if (children.length > 0) {
        fields.push(`/First ${itemIds[children[0].j]} 0 R`);
        fields.push(`/Last ${itemIds[children.at(-1).j]} 0 R`);
      }
    }
    return `<< ${fields.join(' ')} >>`;
  });
  items.forEach((body, i) => {
    objects[itemIds[i] - 1] = body;
  });

  objects[catalogId - 1] =
    `<< /Type /Catalog /Pages ${pagesId} 0 R /Outlines ${outlinesId} 0 R >>`;
  objects[pagesId - 1] =
    `<< /Type /Pages /Count ${PDF_PAGES} /Kids [${pageIds.map(id => `${id} 0 R`).join(' ')}] >>`;
  const rootItems = chapters
    .map((c, j) => ({ c, j }))
    .filter(({ c }) => c.parent === 'root');
  objects[outlinesId - 1] =
    `<< /Type /Outlines /First ${itemIds[rootItems[0].j]} 0 R /Last ${itemIds[rootItems.at(-1).j]} 0 R /Count ${chapters.length} >>`;

  const infoId = add(
    `<< /Title <${utf16beHex('测试PDF样书')}> /Author (gen-assets) >>`,
  );

  // 拼装文件并记录 xref 偏移
  let pdf = '%PDF-1.5\n';
  const offsets = [0];
  objects.forEach((body, i) => {
    offsets.push(pdf.length);
    pdf += `${i + 1} 0 obj\n${body}\nendobj\n`;
  });
  const xrefStart = pdf.length;
  pdf += `xref\n0 ${objects.length + 1}\n`;
  pdf += '0000000000 65535 f \r\n';
  for (let i = 1; i <= objects.length; i++) {
    pdf += `${String(offsets[i]).padStart(10, '0')} 00000 n \r\n`;
  }
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root ${catalogId} 0 R /Info ${infoId} 0 R >>\nstartxref\n${xrefStart}\n%%EOF\n`;
  return Buffer.from(pdf, 'latin1');
}

const novel = buildNovel();
writeFileSync(join(outDir, 'novel.txt'), novel, 'utf8');
writeFileSync(join(outDir, 'sample.epub'), await buildEpub());
writeFileSync(join(outDir, 'sample.pdf'), buildPdf());
console.log(
  `novel.txt: ${novel.length} chars, sample.epub: ${EPUB_CHAPTERS} chapters, sample.pdf: ${PDF_PAGES} pages`,
);
