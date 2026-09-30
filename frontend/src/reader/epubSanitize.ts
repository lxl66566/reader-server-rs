// epub.js renders chapters with the HTML parser, which does not honor
// self-closing syntax on raw-text elements (script/style/textarea/title).
// XHTML books writing e.g. `<script src="x.js"/>` (valid XML) end up with the
// rest of the chapter swallowed as script text, rendering blank pages.
// Expand such tags into explicit empty pairs before epub.js parses the archive.

import JSZip from 'jszip';

const SELF_CLOSED_RAW_TEXT_RE =
  /<(?<tag>script|style|textarea|title)\b(?<attrs>[^>]*?)\/>/gi;

const HTML_ENTRY_RE = /\.x?html?$/i;

/**
 * Expand self-closed raw-text tags in all HTML entries of an epub archive.
 * Returns the original buffer untouched when nothing needed fixing.
 */
export async function sanitizeEpubArchive(
  data: ArrayBuffer,
): Promise<ArrayBuffer> {
  const zip = await JSZip.loadAsync(data);
  const entries = Object.entries(zip.files).filter(
    ([name, entry]) => !entry.dir && HTML_ENTRY_RE.test(name),
  );
  const texts = await Promise.all(
    entries.map(([, entry]) => entry.async('string')),
  );
  const expanded = texts.map(text =>
    text.replace(SELF_CLOSED_RAW_TEXT_RE, '<$<tag>$<attrs>></$<tag>>'),
  );
  const changed = expanded.some((text, i) => text !== texts[i]);
  if (!changed) return data;
  expanded.forEach((text, i) => zip.file(entries[i][0], text));
  return zip.generateAsync({ type: 'arraybuffer' });
}
