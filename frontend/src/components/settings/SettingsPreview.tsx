// Live preview mirroring the current reading settings via inline styles.

import {
  createEffect,
  createSignal,
  For,
  type ParentComponent,
} from 'solid-js';

import type { ReadingSettings } from '../../api/types';
import { fontFamilyCss } from '../../stores/settings';
import {
  convertChinese,
  convertDirection,
  loadChineseConverter,
} from '../../utils/opencc';

export const SettingsPreview: ParentComponent<{
  settings: ReadingSettings;
  /** 预览文案，空行（\n\n）分段 */
  text: string;
}> = props => {
  // bump version after converter loaded so paragraphs recompute
  const [converterVersion, setConverterVersion] = createSignal(0);
  createEffect(() => {
    const direction = convertDirection(props.settings.chinese_conversion);
    void loadChineseConverter(direction).then(() =>
      setConverterVersion(v => v + 1),
    );
  });

  const paragraphs = () => {
    converterVersion();
    const direction = convertDirection(props.settings.chinese_conversion);
    return props.text
      .split(/\n{2,}/)
      .map(p => convertChinese(p.trim(), direction))
      .filter(p => p.length > 0);
  };

  return (
    // 列宽跟随阅读宽度（em 按阅读字号换算为 px）并居中，封顶卡片内容宽
    <div
      class="border-edge mx-auto rounded border p-4"
      style={{
        'background-color': props.settings.background_color,
        color: props.settings.text_color,
        'font-size': `${props.settings.font_size}px`,
        'letter-spacing': `${props.settings.letter_spacing}px`,
        'line-height': props.settings.line_height,
        'max-width': `min(${props.settings.reading_width * props.settings.font_size}px, 100%)`,
      }}
    >
      <For each={paragraphs()}>
        {(paragraph, index) => (
          <p
            style={{
              'font-family':
                fontFamilyCss(props.settings.font_family) || undefined,
              // 段间距只作用于段落之间，末段不留尾距
              'margin-bottom':
                index() === paragraphs().length - 1
                  ? undefined
                  : `${props.settings.paragraph_spacing}em`,
              'text-indent': `${props.settings.text_indent}em`,
            }}
          >
            {paragraph}
          </p>
        )}
      </For>
    </div>
  );
};
