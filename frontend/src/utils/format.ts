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

export function formatPercent(ratio: number): string {
  return (ratio * 100).toFixed(1);
}
