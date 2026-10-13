// 统一的错误提示：ApiError 展示服务端消息（429 用限流文案），其余回退通用文案。

import toast from 'solid-toast';

import { ApiError } from '../api/client';
import type { DictKey } from '../i18n/zh-CN';

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

export function toastError(err: unknown, t: Translate): void {
  toast.error(apiErrorMessage(err, t));
}
