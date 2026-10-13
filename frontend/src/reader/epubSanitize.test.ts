import JSZip from 'jszip';
import { describe, expect, it } from 'vitest';

import { sanitizeEpubArchive } from './epubSanitize';

async function buildEpub(
  entries: Record<string, string>,
): Promise<ArrayBuffer> {
  const zip = new JSZip();
  for (const [name, content] of Object.entries(entries)) {
    zip.file(name, content);
  }
  return zip.generateAsync({ type: 'arraybuffer' });
}

async function readEntry(
  data: ArrayBuffer,
  name: string,
): Promise<string | undefined> {
  const zip = await JSZip.loadAsync(data);
  return zip.file(name)?.async('string');
}

describe('sanitizeEpubArchive', () => {
  it('expands self-closed raw-text tags in html entries', async () => {
    const data = await buildEpub({
      'OPS/ch1.html':
        '<html><head><script src="insertSpan.js" type="text/javascript"/></head>' +
        '<body><h1>第一章</h1><p>正文</p></body></html>',
      'OPS/ch2.xhtml': '<html><body><style src="a.css"/><p>2</p></body></html>',
      'OPS/keep.txt': '<p>纯文本不受影响</p>',
    });

    const fixed = await sanitizeEpubArchive(data);

    expect(await readEntry(fixed, 'OPS/ch1.html')).toContain(
      '<script src="insertSpan.js" type="text/javascript"></script>',
    );
    expect(await readEntry(fixed, 'OPS/ch2.xhtml')).toContain(
      '<style src="a.css"></style>',
    );
    // 非 html 条目保持原样
    expect(await readEntry(fixed, 'OPS/keep.txt')).toBe(
      '<p>纯文本不受影响</p>',
    );
  });

  it('leaves already-closed and unrelated self-closed tags alone', async () => {
    const data = await buildEpub({
      'OPS/ch1.html':
        '<html><head><script></script></head><body><div/><p>ok</p></body></html>',
    });

    const fixed = await sanitizeEpubArchive(data);

    expect(await readEntry(fixed, 'OPS/ch1.html')).toBe(
      '<html><head><script></script></head><body><div/><p>ok</p></body></html>',
    );
  });

  it('returns the original buffer when nothing needs fixing', async () => {
    const data = await buildEpub({
      'OPS/ch1.html': '<html><body><p>ok</p></body></html>',
    });

    await expect(sanitizeEpubArchive(data)).resolves.toBe(data);
  });
  it('strips @font-face rules with non-relative urls in css and inline styles', async () => {
    const data = await buildEpub({
      'OPS/ch1.html':
        '<html><head><style>@font-face{font-family:X;src:url(http://x/y.ttf)}' +
        '.keep{color:#333}</style></head><body><p>ok</p></body></html>',
      'OPS/style.css':
        '@font-face { font-family: A; src: url(file:///C:/win/font.ttf); }\n' +
        '@font-face { font-family: B; src: url("res:///system/font.otf") format("opentype"); }\n' +
        '@font-face { font-family: C; src: url(https://cdn.example.com/f.woff2); }\n' +
        "@font-face { font-family: D; src: url('fonts/inner.woff') format('woff'); }\n" +
        '@font-face { font-family: E; src: url(data:font/woff2;base64,AAA); }\n' +
        'p { color: red; }',
    });

    const fixed = await sanitizeEpubArchive(data);
    const css = await readEntry(fixed, 'OPS/style.css');

    expect(css).not.toContain('file:');
    expect(css).not.toContain('res:');
    expect(css).not.toContain('cdn.example.com');
    // 相对路径与 data: 内嵌字体保留
    expect(css).toContain("url('fonts/inner.woff')");
    expect(css).toContain('url(data:font/woff2;base64,AAA)');
    expect(css).toContain('p { color: red; }');

    const html = await readEntry(fixed, 'OPS/ch1.html');
    expect(html).not.toContain('http://x/y.ttf');
    expect(html).toContain('.keep{color:#333}');
  });
});
