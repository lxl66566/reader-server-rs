// Chinese script conversion, lazily loaded (opencc-js is heavy).
// chinese_conversion=simplified -> t2s（原文繁体转简体展示）；
// traditional -> s2t（反之）；none -> 不转换（null 方向直通）。

import type { ChineseConversion } from '../api/types';

export type ConvertDirection = 's2t' | 't2s';
type ConvertFn = (text: string) => string;

const converters = new Map<ConvertDirection, ConvertFn>();
const loadings = new Map<ConvertDirection, Promise<ConvertFn>>();

/** Sync convert after loadChineseConverter() resolved; passthrough before. */
export function convertChinese(
  text: string,
  direction: ConvertDirection | null,
): string {
  if (direction === null) return text;
  return converters.get(direction)?.(text) ?? text;
}

/** 设置项对应的转换方向；'none' 时返回 null（不转换） */
export function convertDirection(
  mode: ChineseConversion,
): ConvertDirection | null {
  switch (mode) {
    case 'none':
      return null;
    case 'simplified':
      return 't2s';
    case 'traditional':
      return 's2t';
  }
}

export function loadChineseConverter(
  direction: ConvertDirection | null,
): Promise<ConvertFn> {
  if (direction === null) return Promise.resolve((text: string) => text);
  const cached = converters.get(direction);
  if (cached) return Promise.resolve(cached);
  let loading = loadings.get(direction);
  if (!loading) {
    loading = import('opencc-js').then(mod => {
      const converter = mod.Converter(
        direction === 't2s'
          ? { from: 'tw', to: 'cn' }
          : { from: 'cn', to: 'tw' },
      );
      converters.set(direction, converter);
      return converter;
    });
    loadings.set(direction, loading);
  }
  return loading;
}
