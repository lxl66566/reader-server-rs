// Live preview mirroring the current reading settings via inline styles.

import type { ParentComponent } from 'solid-js';
import type { ReadingSettings } from '../../api/types';
import { fontFamilyCss } from '../../stores/settings';

export const SettingsPreview: ParentComponent<{
  settings: ReadingSettings;
  text: string;
}> = props => (
  <div
    class="rounded border border-edge p-4"
    style={{
      'background-color': props.settings.background_color,
      color: props.settings.text_color,
      'font-size': `${props.settings.font_size}px`,
      'letter-spacing': `${props.settings.letter_spacing}px`,
      'line-height': props.settings.line_height,
    }}
  >
    <p
      style={{
        'font-family': fontFamilyCss(props.settings.font_family) || undefined,
        'margin-bottom': `${props.settings.paragraph_spacing}em`,
        'text-indent': `${props.settings.text_indent}em`,
      }}
    >
      {props.text}
    </p>
  </div>
);
