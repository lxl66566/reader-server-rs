// Invite codes: list with copy-to-clipboard and a generate form.

import { VsAdd, VsCopy } from 'solid-icons/vs';
import {
  createResource,
  createSignal,
  For,
  Show,
  type Component,
} from 'solid-js';
import toast from 'solid-toast';

import { adminApi } from '../../api';
import type { InviteCodeItem } from '../../api/types';
import { useI18n } from '../../i18n';
import { formatDateTime } from '../../utils/format';
import { Button, EmptyState, Input, SectionCard, Spinner } from '../ui';
import { isUnauthorized, toastError } from './utils';

export const InviteCodesTab: Component<{
  onUnauthorized: () => void;
}> = props => {
  const { locale, t } = useI18n();
  const [limit, setLimit] = createSignal(1);
  const [description, setDescription] = createSignal('');
  const [busy, setBusy] = createSignal(false);
  const [codes, { refetch }] = createResource(async () => {
    try {
      const res = await adminApi.inviteCodes();
      return res.invite_codes;
    } catch (err) {
      if (isUnauthorized(err)) props.onUnauthorized();
      throw err;
    }
  });
  const list = () => codes() ?? [];

  const copy = (code: string) => {
    void navigator.clipboard
      .writeText(code)
      .then(() => toast.success(t('copied')))
      .catch(() => toast.error(t('copy')));
  };

  const generate = async () => {
    setBusy(true);
    try {
      await adminApi.createInviteCode(limit(), description() || undefined);
      setLimit(1);
      setDescription('');
      toast.success(t('createInviteCode'));
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
      <SectionCard title={t('createInviteCode')}>
        <div class="flex flex-col gap-3 sm:flex-row sm:items-end">
          <label class="flex flex-col gap-1 text-sm">
            <span class="text-muted">{t('limitTimes')}</span>
            <Input
              onInput={value => setLimit(parseLimit(value))}
              type="number"
              value={String(limit())}
            />
          </label>
          <label class="flex flex-1 flex-col gap-1 text-sm">
            <span class="text-muted">{t('descriptionLabel')}</span>
            <Input
              onInput={setDescription}
              placeholder={t('descriptionLabel')}
              value={description()}
            />
          </label>
          <Button
            class="shrink-0"
            disabled={busy()}
            onClick={() => void generate()}
          >
            {busy() ? <Spinner class="size-4" /> : <VsAdd class="size-4" />}
            {t('createInviteCode')}
          </Button>
        </div>
      </SectionCard>

      <SectionCard title={t('adminTabInvite')}>
        <Show
          fallback={<EmptyState text={t('none')} />}
          when={list().length > 0}
        >
          <div class="overflow-x-auto">
            <table class="w-full min-w-[560px] text-sm">
              <thead>
                <tr class="text-muted text-left">
                  <th class="pr-3 pb-2 font-normal">{t('inviteCode')}</th>
                  <th class="pr-3 pb-2 font-normal">{t('limitTimes')}</th>
                  <th class="pr-3 pb-2 font-normal">{t('usedTimes')}</th>
                  <th class="pr-3 pb-2 font-normal">{t('descriptionLabel')}</th>
                  <th class="pb-2 font-normal">{t('createdAt')}</th>
                </tr>
              </thead>
              <tbody>
                <For each={list()}>
                  {(item: InviteCodeItem) => (
                    <tr class="border-edge border-t">
                      <td class="py-2 pr-3">
                        <button
                          class="text-accent flex max-w-52 items-center gap-1 hover:underline"
                          onClick={() => {
                            copy(item.invite_code);
                          }}
                          type="button"
                        >
                          <VsCopy class="size-3.5 shrink-0" />
                          <span class="font-mono text-xs break-all">
                            {item.invite_code}
                          </span>
                        </button>
                      </td>
                      <td class="py-2 pr-3">{item.limit_times}</td>
                      <td class="py-2 pr-3">{item.used_times}</td>
                      <td class="text-muted py-2 pr-3">
                        {item.description ?? t('none')}
                      </td>
                      <td class="text-muted py-2">
                        {formatDateTime(item.created_at, locale())}
                      </td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </div>
        </Show>
      </SectionCard>
    </div>
  );
};

// number inputs emit NaN while clearing; clamp to a valid lower bound.
function parseLimit(value: string): number {
  const n = Number(value);
  return Number.isFinite(n) && n >= 1 ? Math.floor(n) : 1;
}
