import type { DictKey } from '../i18n/zh-CN';

export function formatDateTime(iso: string, locale: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString(locale, {
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    month: '2-digit',
    year: 'numeric',
  });
}

// Formats seconds into a localized, human-friendly duration.
export function formatDuration(
  seconds: number,
  t: (key: DictKey, params?: Record<string, number | string>) => string,
): string {
  const total = Math.round(seconds);
  if (total < 60) return t('timeSeconds', { n: total });
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return t('timeMinutes', { n: minutes });
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (rest === 0) return t('timeHours', { n: hours });
  return t('timeHoursMinutes', { h: hours, m: rest });
}

// Formats a byte count into a compact human-readable size (B/KB/MB/...).
export function formatFileSize(bytes: number): string {
  let value = bytes;
  let unit = 'B';
  for (const next of ['KB', 'MB', 'GB', 'TB'] as const) {
    if (value < 1024) break;
    value /= 1024;
    unit = next;
  }
  const rounded =
    unit === 'B' || value >= 100
      ? Math.round(value)
      : Math.round(value * 10) / 10;
  return `${rounded} ${unit}`;
}

export function formatPercent(ratio: number): string {
  return (ratio * 100).toFixed(1);
}
