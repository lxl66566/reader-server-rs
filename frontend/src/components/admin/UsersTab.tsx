// User management: ban/unban, force sign-out and password reset.

import {
  createResource,
  createSignal,
  For,
  Show,
  type Component,
} from 'solid-js';
import toast from 'solid-toast';

import { adminApi } from '../../api';
import type { AdminUserItem } from '../../api/types';
import { useI18n } from '../../i18n';
import { saveResponseFile } from '../../utils/download';
import { formatDateTime, formatDuration } from '../../utils/format';
import { toastError } from '../../utils/toast';
import { Button, EmptyState, Input, Modal, SectionCard, Spinner } from '../ui';
import { ConfirmDialog } from './ConfirmDialog';
import { isUnauthorized } from './utils';

type PendingAction =
  | { kind: 'ban'; user: AdminUserItem }
  | { kind: 'reset'; user: AdminUserItem }
  | { kind: 'revoke'; user: AdminUserItem }
  | { kind: 'unban'; user: AdminUserItem };

export const UsersTab: Component<{ onUnauthorized: () => void }> = props => {
  const { locale, t } = useI18n();
  const [pending, setPending] = createSignal<null | PendingAction>(null);
  const [newPassword, setNewPassword] = createSignal('');
  const [busy, setBusy] = createSignal(false);
  const [exportingId, setExportingId] = createSignal<null | number>(null);
  const [importOpen, setImportOpen] = createSignal(false);
  const [importFile, setImportFile] = createSignal<File | null>(null);
  const [importUsername, setImportUsername] = createSignal('');
  const [importPassword, setImportPassword] = createSignal('');
  const [importing, setImporting] = createSignal(false);
  const [users, { refetch }] = createResource(async () => {
    try {
      const res = await adminApi.users();
      return res.users;
    } catch (err) {
      if (isUnauthorized(err)) props.onUnauthorized();
      throw err;
    }
  });
  const list = () => users() ?? [];

  const closePending = () => {
    setPending(null);
    setNewPassword('');
  };

  const closeImport = () => {
    setImportOpen(false);
    setImportFile(null);
    setImportUsername('');
    setImportPassword('');
  };

  const onImportFileChange = (event: Event) => {
    const input = event.currentTarget as HTMLInputElement;
    setImportFile(input.files?.[0] ?? null);
  };

  // 导出用户全部数据（含书籍文件）为 zip 下载
  const downloadExport = async (user: AdminUserItem) => {
    setExportingId(user.user_id);
    try {
      const res = await adminApi.exportUser(user.user_id);
      await saveResponseFile(res, `${user.username}.zip`);
    } catch (err) {
      if (isUnauthorized(err)) props.onUnauthorized();
      toastError(err, t);
    } finally {
      setExportingId(null);
    }
  };

  const submitImport = async () => {
    const file = importFile();
    if (!file) return;
    setImporting(true);
    try {
      const form = new FormData();
      form.append('file', file);
      const username = importUsername().trim();
      const password = importPassword();
      if (username) form.append('username', username);
      if (password) form.append('password', password);
      await adminApi.importUser(form);
      toast.success(t('importSuccess'));
      closeImport();
      void refetch();
    } catch (err) {
      if (isUnauthorized(err)) props.onUnauthorized();
      toastError(err, t);
    } finally {
      setImporting(false);
    }
  };

  const runAction = async () => {
    const current = pending();
    if (!current) return;
    setBusy(true);
    try {
      switch (current.kind) {
        case 'ban':
          await adminApi.ban(current.user.user_id);
          toast.success(t('banUser'));
          break;
        case 'reset':
          await adminApi.resetPassword(current.user.user_id, newPassword());
          toast.success(t('resetPassword'));
          break;
        case 'revoke':
          await adminApi.revokeTokens(current.user.user_id);
          toast.success(t('revokeTokens'));
          break;
        case 'unban':
          await adminApi.unban(current.user.user_id);
          toast.success(t('unbanUser'));
          break;
      }
      closePending();
      void refetch();
    } catch (err) {
      if (isUnauthorized(err)) props.onUnauthorized();
      toastError(err, t);
    } finally {
      setBusy(false);
    }
  };

  const actionTitle = () => {
    const current = pending();
    if (!current) return '';
    switch (current.kind) {
      case 'ban':
        return t('banUser');
      case 'reset':
        return t('resetPassword');
      case 'revoke':
        return t('revokeTokens');
      case 'unban':
        return t('unbanUser');
    }
  };

  return (
    <SectionCard
      actions={
        <Button onClick={() => setImportOpen(true)} variant="ghost">
          {importing() ? <Spinner class="size-4" /> : null}
          {t('importUser')}
        </Button>
      }
      title={t('adminTabUsers')}
    >
      <Show fallback={<EmptyState text={t('none')} />} when={list().length > 0}>
        <div class="overflow-x-auto">
          <table class="w-full min-w-[640px] text-sm">
            <thead>
              <tr class="text-muted text-left">
                <th class="pr-3 pb-2 font-normal">{t('username')}</th>
                <th class="pr-3 pb-2 font-normal">{t('createdAt')}</th>
                <th class="pr-3 pb-2 font-normal">{t('bookCount')}</th>
                <th class="pr-3 pb-2 font-normal">{t('readingTime')}</th>
                <th class="pb-2 font-normal">{t('actions')}</th>
              </tr>
            </thead>
            <tbody>
              <For each={list()}>
                {(item: AdminUserItem) => (
                  <tr class="border-edge border-t align-middle">
                    <td class="py-2 pr-3">
                      <span class="font-medium">{item.username}</span>
                      <StatusBadge status={item.status} />
                    </td>
                    <td class="text-muted py-2 pr-3">
                      {formatDateTime(item.created_at, locale())}
                    </td>
                    <td class="py-2 pr-3">{item.book_count}</td>
                    <td class="py-2 pr-3">
                      {formatDuration(item.total_reading_time, t)}
                    </td>
                    <td class="py-2">
                      <div class="flex flex-wrap gap-1">
                        <ActionButton
                          disabled={exportingId() === item.user_id}
                          label={
                            exportingId() === item.user_id
                              ? t('loading')
                              : t('exportUserData')
                          }
                          onClick={() => void downloadExport(item)}
                        />
                        <ActionButton
                          label={t('resetPassword')}
                          onClick={() =>
                            setPending({ kind: 'reset', user: item })
                          }
                        />
                        <ActionButton
                          label={t('revokeTokens')}
                          onClick={() =>
                            setPending({ kind: 'revoke', user: item })
                          }
                        />
                        <Show
                          fallback={
                            <ActionButton
                              label={t('unbanUser')}
                              onClick={() =>
                                setPending({ kind: 'unban', user: item })
                              }
                            />
                          }
                          when={item.status === 'active'}
                        >
                          <ActionButton
                            label={t('banUser')}
                            onClick={() =>
                              setPending({ kind: 'ban', user: item })
                            }
                          />
                        </Show>
                      </div>
                    </td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </div>
      </Show>

      <Show when={pending()?.kind === 'reset'}>
        <Modal onClose={closePending} open title={actionTitle()}>
          <p class="text-muted mb-3 text-sm">
            {pending()?.user.username ?? ''}
          </p>
          <label class="mb-4 flex flex-col gap-1 text-sm">
            <span class="text-muted">{t('newPassword')}</span>
            <Input
              onInput={setNewPassword}
              type="password"
              value={newPassword()}
            />
          </label>
          <div class="flex justify-end gap-2">
            <Button disabled={busy()} onClick={closePending} variant="ghost">
              {t('cancel')}
            </Button>
            <Button
              disabled={busy() || !newPassword()}
              onClick={() => void runAction()}
            >
              {busy() ? <Spinner class="size-4" /> : null}
              {t('confirm')}
            </Button>
          </div>
        </Modal>
      </Show>
      <Modal onClose={closeImport} open={importOpen()} title={t('importUser')}>
        <p class="text-muted mb-3 text-sm">{t('importUserDesc')}</p>
        <label class="border-edge bg-page text-muted hover:bg-surface mb-3 block cursor-pointer rounded border px-3 py-2 text-sm">
          <span class="block truncate">
            {importFile() ? importFile()!.name : t('importSelectFile')}
          </span>
          <input
            accept=".zip"
            class="hidden"
            onChange={onImportFileChange}
            type="file"
          />
        </label>
        <label class="mb-3 flex flex-col gap-1 text-sm">
          <span class="text-muted">{t('username')}</span>
          <Input
            onInput={setImportUsername}
            placeholder={t('importUsernamePlaceholder')}
            value={importUsername()}
          />
        </label>
        <label class="mb-4 flex flex-col gap-1 text-sm">
          <span class="text-muted">{t('password')}</span>
          <Input
            onInput={setImportPassword}
            placeholder={t('importPasswordPlaceholder')}
            type="password"
            value={importPassword()}
          />
        </label>
        <div class="flex justify-end gap-2">
          <Button disabled={importing()} onClick={closeImport} variant="ghost">
            {t('cancel')}
          </Button>
          <Button
            disabled={importing() || !importFile()}
            onClick={() => void submitImport()}
          >
            {importing() ? <Spinner class="size-4" /> : null}
            {t('confirm')}
          </Button>
        </div>
      </Modal>

      <ConfirmDialog
        message={t('confirmAction')}
        onClose={closePending}
        onConfirm={() => void runAction()}
        open={pending() !== null && pending()?.kind !== 'reset'}
        title={actionTitle()}
      />
    </SectionCard>
  );
};

const StatusBadge: Component<{ status: AdminUserItem['status'] }> = props => {
  const { t } = useI18n();
  return (
    <span
      class={`ml-2 rounded px-1.5 py-0.5 text-xs ${
        props.status === 'banned'
          ? 'border-edge bg-surface text-muted border'
          : 'bg-accent/15 text-accent'
      }`}
    >
      {props.status === 'banned' ? t('statusBanned') : t('statusActive')}
    </span>
  );
};

const ActionButton: Component<{
  disabled?: boolean;
  label: string;
  onClick: () => void;
}> = props => (
  <button
    class="border-edge text-muted hover:bg-page hover:text-ink cursor-pointer rounded border px-2 py-1 text-xs disabled:cursor-not-allowed disabled:opacity-50"
    disabled={props.disabled}
    onClick={() => {
      props.onClick();
    }}
    type="button"
  >
    {props.label}
  </button>
);
