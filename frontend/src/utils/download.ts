// Blob 下载工具：从 Response 的 Content-Disposition 提取文件名（失败时用
// fallback），经临时 <a download> 触发浏览器保存。

// Extracts a download filename from a Content-Disposition header; supports the
// filename*=UTF-8'' form. Returns null when absent/malformed.
export function filenameFromContentDisposition(
  value: null | string,
): null | string {
  if (!value) return null;
  const match = /filename\*=UTF-8''([^;]+)|filename="?([^";]+)"?/i.exec(value);
  if (!match) return null;
  const raw = match[1] ?? match[2];
  if (!raw) return null;
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

// 替换 Windows/Linux 文件名非法字符与控制字符，避免浏览器静默改名或跨端异常
export function sanitizeFilename(name: string): string {
  const cleaned = Array.from(name, ch => {
    const code = ch.codePointAt(0) ?? 0;
    return code <= 0x1f || '\\/:*?"<>|'.includes(ch) ? '_' : ch;
  })
    .join('')
    .trim();
  return cleaned || 'download';
}

// 把已就绪的 Response 保存为本地文件，返回实际使用的文件名
export async function saveResponseFile(
  res: Response,
  fallbackName: string,
): Promise<string> {
  const blob = await res.blob();
  const filename = sanitizeFilename(
    filenameFromContentDisposition(res.headers.get('Content-Disposition')) ??
      fallbackName,
  );
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
  return filename;
}
