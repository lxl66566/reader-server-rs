// Shared admin helpers: error rendering + 401 detection.

import toast from 'solid-toast';

import { ApiError } from '../../api/client';
import type { DictKey } from '../../i18n/zh-CN';

export type Translate = (
  key: DictKey,
  params?: Record<string, number | string>,
) => string;

export function apiErrorMessage(err: unknown, t: Translate): string {
  if (err instanceof ApiError) {
    return err.status === 429 ? t('errorRateLimit') : err.message;
  }
  return t('errorGeneric');
}

// Admin tokens live in a separate namespace; any 401 means the session is dead.
export function isUnauthorized(err: unknown): boolean {
  return err instanceof ApiError && err.status === 401;
}

export function toastError(err: unknown, t: Translate): void {
  toast.error(apiErrorMessage(err, t));
}
