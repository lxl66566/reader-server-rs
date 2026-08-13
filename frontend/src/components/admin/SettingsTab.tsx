// Global settings: invite-code requirement toggle + full backup download.

import { VsDownload } from 'solid-icons/vs';
import {
  createEffect,
  createResource,
  createSignal,
  type Component,
} from 'solid-js';
import toast from 'solid-toast';

import { adminApi } from '../../api';
import { useI18n } from '../../i18n';
import { Button, SectionCard, Spinner, Toggle } from '../ui';
import { isUnauthorized, toastError } from './utils';

export const SettingsTab: Component<{ onUnauthorized: () => void }> = props => {
  const { t } = useI18n();
  const [settings, { refetch }] = createResource(() => adminApi.settings());
  const [required, setRequired] = createSignal<boolean | undefined>(undefined);
  const [backingUp, setBackingUp] = createSignal(false);

  // an expired admin token surfaces as 401 on any request
  createEffect(() => {
    if (isUnauthorized(settings.error)) props.onUnauthorized();
  });

  // keep the toggle in sync once the server value loads
  createEffect(() => {
    const data = settings();
    if (data) setRequired(data.invite_code_required);
  });

  const toggleRequired = (value: boolean) => {
    setRequired(value);
    void adminApi
      .updateSettings(value)
      .then(() => {
        toast.success(t('save'));
        void refetch();
        return null;
      })
      .catch((err: unknown) => {
        setRequired(settings()?.invite_code_required ?? false);
        toastError(err, t);
      });
  };

  const downloadBackup = async () => {
    setBackingUp(true);
    try {
      const res = await adminApi.backup();
      const blob = await res.blob();
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = 'backup.zip';
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(url);
      toast.success(t('downloadBackup'));
    } catch (err) {
      if (isUnauthorized(err)) props.onUnauthorized();
      toastError(err, t);
    } finally {
      setBackingUp(false);
    }
  };

  return (
    <div class="space-y-4">
      <SectionCard title={t('inviteCodeRequired')}>
        <div class="flex items-center justify-between">
          <span class="text-sm">{t('inviteCodeRequired')}</span>
          <Toggle checked={required() ?? false} onChange={toggleRequired} />
        </div>
      </SectionCard>

      <SectionCard title={t('downloadBackup')}>
        <p class="text-muted mb-3 text-sm">{t('downloadBackupDesc')}</p>
        <Button disabled={backingUp()} onClick={() => void downloadBackup()}>
          {backingUp() ? (
            <Spinner class="size-4" />
          ) : (
            <VsDownload class="size-4" />
          )}
          {t('downloadBackup')}
        </Button>
      </SectionCard>
    </div>
  );
};
