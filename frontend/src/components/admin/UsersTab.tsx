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
import { formatDateTime, formatDuration } from '../../utils/format';
import { Button, EmptyState, Input, Modal, SectionCard, Spinner } from '../ui';
import { ConfirmDialog } from './ConfirmDialog';
import { isUnauthorized, toastError } from './utils';

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
    <SectionCard title={t('adminTabUsers')}>
      <Show fallback={<EmptyState text={t('none')} />} when={list().length > 0}>
        <div class="overflow-x-auto">
          <table class="w-full min-w-[640px] text-sm">
            <thead>
              <tr class="text-left text-muted">
                <th class="pb-2 pr-3 font-normal">{t('username')}</th>
                <th class="pb-2 pr-3 font-normal">{t('createdAt')}</th>
                <th class="pb-2 pr-3 font-normal">{t('bookCount')}</th>
                <th class="pb-2 pr-3 font-normal">{t('readingTime')}</th>
                <th class="pb-2 font-normal">{t('actions')}</th>
              </tr>
            </thead>
            <tbody>
              <For each={list()}>
                {(item: AdminUserItem) => (
                  <tr class="border-t border-edge align-middle">
                    <td class="py-2 pr-3">
                      <span class="font-medium">{item.username}</span>
                      <StatusBadge status={item.status} />
                    </td>
                    <td class="py-2 pr-3 text-muted">
                      {formatDateTime(item.created_at, locale())}
                    </td>
                    <td class="py-2 pr-3">{item.book_count}</td>
                    <td class="py-2 pr-3">
                      {formatDuration(item.total_reading_time, t)}
                    </td>
                    <td class="py-2">
                      <div class="flex flex-wrap gap-1">
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
          <p class="mb-3 text-sm text-muted">
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
          ? 'border border-edge bg-surface text-muted'
          : 'bg-accent/15 text-accent'
      }`}
    >
      {props.status === 'banned' ? t('statusBanned') : t('statusActive')}
    </span>
  );
};

const ActionButton: Component<{
  label: string;
  onClick: () => void;
}> = props => (
  <button
    class="cursor-pointer rounded border border-edge px-2 py-1 text-xs text-muted hover:bg-base hover:text-ink"
    onClick={() => {
      props.onClick();
    }}
    type="button"
  >
    {props.label}
  </button>
);
