// Settings page: reading preferences with live preview, UI language and
// account actions (change password, data export, account deletion).

import { useNavigate } from '@solidjs/router';
import { VsDownload, VsKey, VsTrash } from 'solid-icons/vs';
import { createSignal, Show, type JSX, type ParentComponent } from 'solid-js';
import toast from 'solid-toast';
import { ApiError, authApi } from '../api';
import type { PaginationMode, ThemeMode } from '../api/types';
import {
  ColorControl,
  RangeControl,
  SelectControl,
  SettingRow,
  StepperControl,
} from '../components/settings/controls';
import { SettingsPreview } from '../components/settings/SettingsPreview';
import {
  Button,
  Input,
  Modal,
  SectionCard,
  Spinner,
  Toggle,
} from '../components/ui';
import { LOCALES, useI18n } from '../i18n';
import type { DictKey } from '../i18n/zh-CN';
import { useAuth, useRequireAuth } from '../stores/auth';
import {
  FONT_FAMILY_OPTIONS,
  PAGINATION_OPTIONS,
  THEME_OPTIONS,
  useSettings,
  type FontFamilyOption,
} from '../stores/settings';
import { formatDuration } from '../utils/format';

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

const Settings = () => {
  useRequireAuth();
  const { locale, setLocale, t } = useI18n();
  const auth = useAuth();
  const navigate = useNavigate();
  const { settings, update } = useSettings();

  const showError = (error: unknown, fallback: DictKey) => {
    toast.error(error instanceof ApiError ? error.message : t(fallback));
  };

  // --- change password modal ---
  const [pwdOpen, setPwdOpen] = createSignal(false);
  const [oldPassword, setOldPassword] = createSignal('');
  const [newPassword, setNewPassword] = createSignal('');
  const [confirmPassword, setConfirmPassword] = createSignal('');
  const [pwdSubmitting, setPwdSubmitting] = createSignal(false);

  const closePwd = () => {
    setPwdOpen(false);
    setOldPassword('');
    setNewPassword('');
    setConfirmPassword('');
  };

  const submitPassword = async () => {
    if (newPassword() !== confirmPassword()) {
      toast.error(t('passwordMismatch'));
      return;
    }
    setPwdSubmitting(true);
    try {
      await authApi.changePassword(oldPassword(), newPassword());
      toast.success(t('changePasswordSuccess'));
      closePwd();
    } catch (error) {
      showError(error, 'errorGeneric');
    } finally {
      setPwdSubmitting(false);
    }
  };

  // --- data export (zip download from a raw Response) ---
  const [exporting, setExporting] = createSignal(false);
  const exportData = async () => {
    setExporting(true);
    try {
      const res = await authApi.exportData();
      const blob = await res.blob();
      const filename =
        filenameFromContentDisposition(
          res.headers.get('Content-Disposition'),
        ) ?? 'export.zip';
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = filename;
      document.body.appendChild(link);
      link.click();
      link.remove();
      URL.revokeObjectURL(url);
    } catch (error) {
      showError(error, 'errorGeneric');
    } finally {
      setExporting(false);
    }
  };

  // --- account deletion modal ---
  const [delOpen, setDelOpen] = createSignal(false);
  const [delPassword, setDelPassword] = createSignal('');
  const [delSubmitting, setDelSubmitting] = createSignal(false);

  const closeDel = () => {
    setDelOpen(false);
    setDelPassword('');
  };

  const submitDelete = async () => {
    setDelSubmitting(true);
    try {
      await authApi.deleteAccount(delPassword());
      auth.logout();
      navigate('/', { replace: true });
    } catch (error) {
      showError(error, 'errorGeneric');
    } finally {
      setDelSubmitting(false);
    }
  };

  return (
    <div class="space-y-4">
      <h1 class="text-lg font-medium">{t('navSettings')}</h1>

      <SectionCard title={t('readerSettings')}>
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
      </SectionCard>

      <SectionCard title={t('settingsPreview')}>
        <SettingsPreview settings={settings} text={t('settingsPreviewText')} />
      </SectionCard>

      <SectionCard title={t('languageLabel')}>
        <SettingRow
          label={t('languageLabel')}
          right={
            <SelectControl
              onChange={value => {
                setLocale(value as (typeof LOCALES)[number]);
              }}
              options={LOCALES.map(code => ({
                label: t(code === 'zh-CN' ? 'languageZhCN' : 'languageEn'),
                value: code,
              }))}
              value={locale()}
            />
          }
        />
      </SectionCard>

      <SectionCard title={t('accountSection')}>
        <Show fallback={<Spinner class="size-5" />} when={auth.user()}>
          {user => (
            <div class="mb-3 border-b border-edge pb-3">
              <InfoRow label={t('username')} value={user().username} />
              <InfoRow
                label={t('readingTime')}
                value={formatDuration(user().total_reading_time, t)}
              />
              <InfoRow
                label={t('bookCount')}
                value={String(user().book_count)}
              />
            </div>
          )}
        </Show>
        <div class="space-y-3">
          <Button class="w-full" onClick={() => setPwdOpen(true)}>
            <VsKey class="size-4" />
            {t('changePassword')}
          </Button>
          <div class="flex items-center justify-between gap-3">
            <div class="min-w-0">
              <div class="text-sm text-ink">{t('exportData')}</div>
              <div class="text-xs text-muted">{t('exportDataDesc')}</div>
            </div>
            <Button
              disabled={exporting()}
              onClick={() => void exportData()}
              variant="ghost"
            >
              <VsDownload class="size-4" />
              {exporting() ? t('loading') : t('exportData')}
            </Button>
          </div>
          <div class="rounded border border-red-300 p-3 dark:border-red-950">
            <div class="flex items-center justify-between gap-3">
              <div class="min-w-0">
                <div class="text-sm text-red-700 dark:text-red-400">
                  {t('deleteAccount')}
                </div>
                <div class="text-xs text-muted">{t('deleteAccountDesc')}</div>
              </div>
              <Button onClick={() => setDelOpen(true)} variant="danger">
                <VsTrash class="size-4" />
                {t('deleteAccount')}
              </Button>
            </div>
          </div>
        </div>
      </SectionCard>

      <Modal onClose={closePwd} open={pwdOpen()} title={t('changePassword')}>
        <form
          class="space-y-3"
          onSubmit={event => {
            event.preventDefault();
            void submitPassword();
          }}
        >
          <FormField
            input={
              <Input
                class="w-full"
                onInput={setOldPassword}
                type="password"
                value={oldPassword()}
              />
            }
            label={t('oldPassword')}
          />
          <FormField
            input={
              <Input
                class="w-full"
                onInput={setNewPassword}
                type="password"
                value={newPassword()}
              />
            }
            label={t('newPassword')}
          />
          <FormField
            input={
              <Input
                class="w-full"
                onInput={setConfirmPassword}
                type="password"
                value={confirmPassword()}
              />
            }
            label={t('confirmPassword')}
          />
          <div class="flex justify-end gap-2 pt-1">
            <Button onClick={closePwd} variant="ghost">
              {t('cancel')}
            </Button>
            <Button disabled={pwdSubmitting()} type="submit">
              {t('confirm')}
            </Button>
          </div>
        </form>
      </Modal>

      <Modal onClose={closeDel} open={delOpen()} title={t('deleteAccount')}>
        <form
          class="space-y-3"
          onSubmit={event => {
            event.preventDefault();
            void submitDelete();
          }}
        >
          <p class="text-sm text-muted">{t('deleteAccountConfirm')}</p>
          <FormField
            input={
              <Input
                class="w-full"
                onInput={setDelPassword}
                type="password"
                value={delPassword()}
              />
            }
            label={t('password')}
          />
          <div class="flex justify-end gap-2 pt-1">
            <Button onClick={closeDel} variant="ghost">
              {t('cancel')}
            </Button>
            <Button disabled={delSubmitting()} type="submit" variant="danger">
              {t('confirm')}
            </Button>
          </div>
        </form>
      </Modal>
    </div>
  );
};

const FormField: ParentComponent<{
  input: JSX.Element;
  label: string;
}> = props => (
  <label class="block space-y-1">
    <span class="text-sm text-muted">{props.label}</span>
    {props.input}
  </label>
);

const InfoRow: ParentComponent<{ label: string; value: string }> = props => (
  <div class="flex items-center justify-between py-1 text-sm">
    <span class="text-muted">{props.label}</span>
    <span>{props.value}</span>
  </div>
);

// Extracts a download filename from a Content-Disposition header; supports the
// filename*=UTF-8'' form. Returns null when absent/malformed.
function filenameFromContentDisposition(value: null | string): null | string {
  if (!value) return null;
  const match = /filename\*=UTF-8''([^;]+)|filename="?([^";]+)"?/i.exec(value);
  if (!match) return null;
  const raw = match[1] ?? match[2];
  if (!raw) return null;
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

export default Settings;
