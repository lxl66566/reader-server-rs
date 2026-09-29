// 阅读设置控件组：阅读页设置弹窗与设置页共用同一组控件行，
// 主题切换只在设置页展示（阅读页弹窗通过 withTheme=false 省略）。

import { Show } from 'solid-js';

import type { PaginationMode, ThemeMode } from '../../api/types';
import { useI18n } from '../../i18n';
import type { DictKey } from '../../i18n/zh-CN';
import {
  FONT_FAMILY_OPTIONS,
  type FontFamilyOption,
  PAGINATION_OPTIONS,
  THEME_OPTIONS,
  useSettings,
} from '../../stores/settings';
import { Toggle } from '../ui';
import {
  ColorControl,
  RangeControl,
  SelectControl,
  SettingRow,
  StepperControl,
} from './controls';

const FONT_LABELS: Record<FontFamilyOption, DictKey> = {
  default: 'fontDefault',
  sans: 'fontSans',
  serif: 'fontSerif',
};

const THEME_LABELS: Record<ThemeMode, DictKey> = {
  dark: 'themeDark',
  light: 'themeLight',
  sepia: 'themeSepia',
  system: 'themeSystem',
};

const PAGINATION_LABELS: Record<PaginationMode, DictKey> = {
  paginate: 'modePaginate',
  scroll: 'modeScroll',
};

// Neutral preset swatches; stored in the DB, not theme-aware.
const BACKGROUND_PRESETS = ['#201f1b', '#eef1f5', '#f8f1e1', '#faf9f6'];
const TEXT_PRESETS = ['#2b2a26', '#4a3f2d', '#5b6b8c', '#e8e6e0'];

export const ReaderSettingRows = (props: { withTheme?: boolean }) => {
  const { t } = useI18n();
  const { settings, update } = useSettings();

  return (
    <>
      <SettingRow
        label={t('settingsFontSize')}
        right={
          <StepperControl
            max={28}
            min={12}
            onChange={value => {
              update({ font_size: value });
            }}
            value={settings.font_size}
          />
        }
      />
      <SettingRow
        label={t('settingsLineHeight')}
        right={
          <RangeControl
            max={2.4}
            min={1.2}
            onChange={value => {
              update({ line_height: value });
            }}
            step={0.1}
            value={settings.line_height}
          />
        }
      />
      <SettingRow
        label={t('settingsLetterSpacing')}
        right={
          <RangeControl
            max={4}
            min={0}
            onChange={value => {
              update({ letter_spacing: value });
            }}
            step={0.5}
            unit="px"
            value={settings.letter_spacing}
          />
        }
      />
      <SettingRow
        label={t('settingsParagraphSpacing')}
        right={
          <RangeControl
            max={2}
            min={0}
            onChange={value => {
              update({ paragraph_spacing: value });
            }}
            step={0.1}
            unit="em"
            value={settings.paragraph_spacing}
          />
        }
      />
      <SettingRow
        label={t('settingsReadingWidth')}
        right={
          <RangeControl
            max={60}
            min={20}
            onChange={value => {
              update({ reading_width: value });
            }}
            step={1}
            unit="em"
            value={settings.reading_width}
          />
        }
      />
      <SettingRow
        label={t('settingsTextIndent')}
        right={
          <SelectControl
            onChange={value => {
              update({ text_indent: Number(value) });
            }}
            options={[
              { label: t('indentNone'), value: '0' },
              { label: t('indentOne'), value: '1' },
              { label: t('indentTwo'), value: '2' },
            ]}
            value={String(settings.text_indent)}
          />
        }
      />
      <SettingRow
        label={t('settingsFontFamily')}
        right={
          <SelectControl
            onChange={value => {
              update({ font_family: value });
            }}
            options={FONT_FAMILY_OPTIONS.map(option => ({
              label: t(FONT_LABELS[option]),
              value: option,
            }))}
            value={settings.font_family}
          />
        }
      />
      <SettingRow
        label={t('settingsBackground')}
        right={
          <ColorControl
            onChange={value => {
              update({ background_color: value });
            }}
            presets={BACKGROUND_PRESETS}
            value={settings.background_color}
          />
        }
      />
      <SettingRow
        label={t('settingsTextColor')}
        right={
          <ColorControl
            onChange={value => {
              update({ text_color: value });
            }}
            presets={TEXT_PRESETS}
            value={settings.text_color}
          />
        }
      />
      <SettingRow
        label={t('settingsSimplified')}
        right={
          <Toggle
            checked={settings.simplified_chinese}
            onChange={checked => {
              update({ simplified_chinese: checked });
            }}
          />
        }
      />
      <Show when={props.withTheme}>
        <SettingRow
          label={t('settingsTheme')}
          right={
            <SelectControl
              onChange={value => {
                update({ theme: value as ThemeMode });
              }}
              options={THEME_OPTIONS.map(option => ({
                label: t(THEME_LABELS[option]),
                value: option,
              }))}
              value={settings.theme}
            />
          }
        />
      </Show>
      <SettingRow
        label={t('settingsPaginationMode')}
        right={
          <SelectControl
            onChange={value => {
              update({ pagination_mode: value as PaginationMode });
            }}
            options={PAGINATION_OPTIONS.map(option => ({
              label: t(PAGINATION_LABELS[option]),
              value: option,
            }))}
            value={settings.pagination_mode}
          />
        }
      />
    </>
  );
};
