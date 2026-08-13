// Admin accounts: list existing admins and add new ones.

import { VsAdd } from 'solid-icons/vs';
import {
  createResource,
  createSignal,
  For,
  Show,
  type Component,
} from 'solid-js';
import toast from 'solid-toast';

import { adminApi } from '../../api';
import type { AdminItem } from '../../api/types';
import { useI18n } from '../../i18n';
import { formatDateTime } from '../../utils/format';
import { Button, EmptyState, Input, SectionCard, Spinner } from '../ui';
import { isUnauthorized, toastError } from './utils';

export const AdminsTab: Component<{ onUnauthorized: () => void }> = props => {
  const { locale, t } = useI18n();
  const [username, setUsername] = createSignal('');
  const [password, setPassword] = createSignal('');
  const [busy, setBusy] = createSignal(false);
  const [admins, { refetch }] = createResource(async () => {
    try {
      const res = await adminApi.admins();
      return res.admins;
    } catch (err) {
      if (isUnauthorized(err)) props.onUnauthorized();
      throw err;
    }
  });
  const list = () => admins() ?? [];

  const addAdmin = async () => {
    setBusy(true);
    try {
      await adminApi.createAdmin(username(), password());
      setUsername('');
      setPassword('');
      toast.success(t('addAdmin'));
      void refetch();
    } catch (err) {
      if (isUnauthorized(err)) props.onUnauthorized();
      toastError(err, t);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="space-y-4">
      <SectionCard title={t('addAdmin')}>
        <div class="flex flex-col gap-3 sm:flex-row sm:items-end">
          <label class="flex flex-1 flex-col gap-1 text-sm">
            <span class="text-muted">{t('username')}</span>
            <Input onInput={setUsername} value={username()} />
          </label>
          <label class="flex flex-1 flex-col gap-1 text-sm">
            <span class="text-muted">{t('password')}</span>
            <Input onInput={setPassword} type="password" value={password()} />
          </label>
          <Button
            class="shrink-0"
            disabled={busy() || !username() || !password()}
            onClick={() => void addAdmin()}
          >
            {busy() ? <Spinner class="size-4" /> : <VsAdd class="size-4" />}
            {t('addAdmin')}
          </Button>
        </div>
      </SectionCard>

      <SectionCard title={t('adminTabAdmins')}>
        <Show
          fallback={<EmptyState text={t('none')} />}
          when={list().length > 0}
        >
          <ul class="divide-edge divide-y">
            <For each={list()}>
              {(item: AdminItem) => (
                <li class="flex items-center justify-between py-2 text-sm">
                  <span class="font-medium">{item.username}</span>
                  <span class="text-muted">
                    {formatDateTime(item.created_at, locale())}
                  </span>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </SectionCard>
    </div>
  );
};
