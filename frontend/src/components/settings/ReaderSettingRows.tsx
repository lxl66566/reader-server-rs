// 阅读设置控件组：阅读页设置弹窗与设置页共用同一组控件行。
// 主题切换在导航栏（ThemeSwitcher），不在此处。

import type { ChineseConversion, PaginationMode } from '../../api/types';
import { useI18n } from '../../i18n';
import type { DictKey } from '../../i18n/zh-CN';
import {
  FONT_FAMILY_OPTIONS,
  type FontFamilyOption,
  PAGINATION_OPTIONS,
  useSettings,
} from '../../stores/settings';
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

const PAGINATION_LABELS: Record<PaginationMode, DictKey> = {
  paginate: 'modePaginate',
  scroll: 'modeScroll',
};

const CONVERSION_LABELS: Record<ChineseConversion, DictKey> = {
  none: 'conversionNone',
  simplified: 'conversionSimplified',
  traditional: 'conversionTraditional',
};

const CONVERSION_OPTIONS: ChineseConversion[] = [
  'none',
  'simplified',
  'traditional',
];
// Neutral preset swatches; stored in the DB, not theme-aware.
const BACKGROUND_PRESETS = ['#201f1b', '#eef1f5', '#f8f1e1', '#c7edcc'];
const TEXT_PRESETS = ['#2b2a26', '#4a3f2d', '#5b6b8c', '#e8e6e0'];

export const ReaderSettingRows = () => {
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
            max={100}
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
        label={t('settingsConversion')}
        right={
          <SelectControl
            onChange={value => {
              update({ chinese_conversion: value as ChineseConversion });
            }}
            options={CONVERSION_OPTIONS.map(option => ({
              label: t(CONVERSION_LABELS[option]),
              value: option,
            }))}
            value={settings.chinese_conversion}
          />
        }
      />
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
