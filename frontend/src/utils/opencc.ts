// Traditional -> Simplified conversion, lazily loaded (opencc-js is heavy).

type ConvertFn = (text: string) => string;

let converter: ConvertFn | null = null;
let loading: null | Promise<ConvertFn> = null;

export function loadChineseConverter(): Promise<ConvertFn> {
  if (converter) return Promise.resolve(converter);
  loading ??= import('opencc-js').then(mod => {
    converter = mod.Converter({ from: 'tw', to: 'cn' });
    return converter;
  });
  return loading;
}

/** Sync convert after loadChineseConverter() resolved; passthrough before. */
export function toSimplified(text: string): string {
  return converter ? converter(text) : text;
}
