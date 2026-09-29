// Live preview mirroring the current reading settings via inline styles.

import { For, type ParentComponent } from 'solid-js';

import type { ReadingSettings } from '../../api/types';
import { fontFamilyCss } from '../../stores/settings';

export const SettingsPreview: ParentComponent<{
  settings: ReadingSettings;
  /** 预览文案，空行（\n\n）分段 */
  text: string;
}> = props => {
  const paragraphs = () =>
    props.text
      .split(/\n{2,}/)
      .map(p => p.trim())
      .filter(p => p.length > 0);

  return (
    <div
      class="border-edge rounded border p-4"
      style={{
        'background-color': props.settings.background_color,
        color: props.settings.text_color,
        'font-size': `${props.settings.font_size}px`,
        'letter-spacing': `${props.settings.letter_spacing}px`,
        'line-height': props.settings.line_height,
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
