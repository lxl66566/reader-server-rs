// epub.js renders chapters with the HTML parser, which does not honor
// self-closing syntax on raw-text elements (script/style/textarea/title).
// XHTML books writing e.g. `<script src="x.js"/>` (valid XML) end up with the
// rest of the chapter swallowed as script text, rendering blank pages.
// Expand such tags into explicit empty pairs before epub.js parses the archive.

import JSZip from 'jszip';

const SELF_CLOSED_RAW_TEXT_RE =
  /<(?<tag>script|style|textarea|title)\b(?<attrs>[^>]*?)\/>/gi;

const HTML_ENTRY_RE = /\.x?html?$/i;
const CSS_ENTRY_RE = /\.css$/i;

/**
 * 引用外部资源的 @font-face 规则（file:/res:/http(s): 等非相对 url）。
 * epub.js 无法解析这类 src，会在 console 持续刷错误；书内嵌字体走
 * 相对路径或 data: URI，不受影响
 */
const EXTERNAL_FONT_FACE_RE =
  /@font-face\s*\{[^}]*?url\(\s*(?:['"]\s*)?(?:file:|res:|https?:)[^}]*\}/gi;

/**
 * Sanitize all HTML / CSS entries of an epub archive:
 * - expand self-closed raw-text tags (HTML only, see above);
 * - strip @font-face rules whose src references non-relative URLs.
 * Returns the original buffer untouched when nothing needed fixing.
 */
export async function sanitizeEpubArchive(
  data: ArrayBuffer,
): Promise<ArrayBuffer> {
  const zip = await JSZip.loadAsync(data);
  const entries = Object.entries(zip.files).filter(
    ([name, entry]) =>
      !entry.dir && (HTML_ENTRY_RE.test(name) || CSS_ENTRY_RE.test(name)),
  );
  const texts = await Promise.all(
    entries.map(([, entry]) => entry.async('string')),
  );
  const sanitized = texts.map(text =>
    text
      .replace(SELF_CLOSED_RAW_TEXT_RE, '<$<tag>$<attrs>></$<tag>>')
      .replace(EXTERNAL_FONT_FACE_RE, ''),
  );
  const changed = sanitized.some((text, i) => text !== texts[i]);
  if (!changed) return data;
  entries.forEach(([name], i) => {
    const text = sanitized[i];
    if (text !== undefined) zip.file(name, text);
  });
  return zip.generateAsync({ type: 'arraybuffer' });
}
