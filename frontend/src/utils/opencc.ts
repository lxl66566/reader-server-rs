// Chinese script conversion, lazily loaded (opencc-js is heavy).
// simplified_chinese=true -> t2s (原文繁体转简体展示), false -> s2t (反之).

export type ConvertDirection = 's2t' | 't2s';
type ConvertFn = (text: string) => string;

const converters = new Map<ConvertDirection, ConvertFn>();
const loadings = new Map<ConvertDirection, Promise<ConvertFn>>();

/** Sync convert after loadChineseConverter() resolved; passthrough before. */
export function convertChinese(
  text: string,
  direction: ConvertDirection,
): string {
  return converters.get(direction)?.(text) ?? text;
}

/** 设置项对应的转换方向 */
export function convertDirection(simplified: boolean): ConvertDirection {
  return simplified ? 't2s' : 's2t';
}

export function loadChineseConverter(
  direction: ConvertDirection,
): Promise<ConvertFn> {
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
