// Settings page: reading preferences with live preview, UI language and
// account actions (change password, data export, account deletion).

import { useNavigate } from '@solidjs/router';
import { VsDownload, VsKey, VsTrash } from 'solid-icons/vs';
import { createSignal, Show, type JSX, type ParentComponent } from 'solid-js';
import toast from 'solid-toast';

import { ApiError, authApi } from '../api';
import { SelectControl, SettingRow } from '../components/settings/controls';
import { ReaderSettingRows } from '../components/settings/ReaderSettingRows';
import { SettingsPreview } from '../components/settings/SettingsPreview';
import {
  Button,
  FullBleed,
  Input,
  Modal,
  SectionCard,
  Spinner,
} from '../components/ui';
import { LOCALES, useI18n } from '../i18n';
import type { DictKey } from '../i18n/zh-CN';
import { useAuth, useRequireAuth } from '../stores/auth';
import { useSettings } from '../stores/settings';
import { formatDuration } from '../utils/format';

const Settings = () => {
  useRequireAuth();
  const { locale, setLocale, t } = useI18n();
  const auth = useAuth();
  const navigate = useNavigate();
  const { settings } = useSettings();

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
      // 后端已递增 token_version：当前 token 立即作废，主动登出避免
      // 后续请求逐个 401 的突兀体验
      toast.success(t('changePasswordSuccess'));
      auth.logout();
      navigate('/login', { replace: true });
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
        <ReaderSettingRows />
      </SectionCard>

      {/* 预览卡宽度跟随阅读宽度（em 按阅读字号换算为 px，卡片层 em 基准不同；
          +2rem 补偿卡片自身 px-4 内边距，使内部文本区恰为阅读宽度）。
          最小值与其他设置卡对齐：Layout main 为 max-w-4xl + px-4，内容宽
          54rem，改动 Layout 时需同步此处；100vw 含滚动条，封顶留 2rem 防
          边框被裁。卡片铺开的是自身，不反撑设置区容器，布局不抖动 */}
      <FullBleed>
        <div
          class="mx-auto w-full"
          style={{
            'max-width': `min(max(54rem, calc(${settings.reading_width * settings.font_size}px + 2rem)), calc(100vw - 2rem))`,
          }}
        >
          <SectionCard title={t('settingsPreview')}>
            <SettingsPreview
              settings={settings}
              text={t('settingsPreviewText')}
            />
          </SectionCard>
        </div>
      </FullBleed>

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
            <div class="border-edge mb-3 border-b pb-3">
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
              <div class="text-ink text-sm">{t('exportData')}</div>
              <div class="text-muted text-xs">{t('exportDataDesc')}</div>
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
                <div class="text-muted text-xs">{t('deleteAccountDesc')}</div>
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
          <p class="text-muted text-sm">{t('deleteAccountConfirm')}</p>
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
    <span class="text-muted text-sm">{props.label}</span>
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
