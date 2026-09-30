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
});
