// Audit logs: paginated read-only list of admin actions.

import { VsChevronLeft, VsChevronRight } from 'solid-icons/vs';
import {
  createEffect,
  createResource,
  createSignal,
  For,
  Show,
  type Component,
} from 'solid-js';

import { adminApi } from '../../api';
import type { AuditLogItem } from '../../api/types';
import { useI18n } from '../../i18n';
import { formatDateTime } from '../../utils/format';
import { Button, EmptyState, SectionCard } from '../ui';
import { isUnauthorized } from './utils';

const PAGE_LIMIT = 20;

export const AuditLogsTab: Component<{
  onUnauthorized: () => void;
}> = props => {
  const { locale, t } = useI18n();
  const [page, setPage] = createSignal(1);
  const [logs] = createResource(page, async current =>
    adminApi.auditLogs(current, PAGE_LIMIT),
  );

  // an expired admin token surfaces as 401 on any request
  createEffect(() => {
    if (isUnauthorized(logs.error)) props.onUnauthorized();
  });

  const hasPrev = () => page() > 1;
  const hasNext = () => (logs()?.total ?? 0) > page() * PAGE_LIMIT;

  const prev = () => {
    if (hasPrev()) setPage(p => p - 1);
  };

  const next = () => {
    if (hasNext()) setPage(p => p + 1);
  };

  return (
    <SectionCard title={t('adminTabLogs')}>
      <Show
        fallback={<EmptyState text={t('none')} />}
        when={(logs()?.logs.length ?? 0) > 0}
      >
        <ul class="space-y-2">
          <For each={logs()?.logs ?? []}>
            {(item: AuditLogItem) => (
              <li class="border-edge bg-base rounded-lg border p-3 text-sm">
                <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
                  <span class="font-medium">{item.admin_username}</span>
                  <span class="text-muted">
                    {formatDateTime(item.created_at, locale())}
                  </span>
                </div>
                <div class="text-muted mt-1">
                  <span class="text-ink">{item.action}</span>
                  <Show
                    when={item.target_type !== null || item.target_id !== null}
                  >
                    {' · '}
                    {item.target_type ?? ''}
                    {item.target_id !== null ? ` #${item.target_id}` : ''}
                  </Show>
                </div>
                <Show when={item.detail}>
                  <p class="text-muted mt-1">{item.detail}</p>
                </Show>
              </li>
            )}
          </For>
        </ul>
        <div class="mt-4 flex items-center justify-between">
          <Button disabled={!hasPrev()} onClick={prev} variant="ghost">
            <VsChevronLeft class="size-4" />
            {t('prevPage')}
          </Button>
          <span class="text-muted text-sm">
            {t('pageIndicator', {
              page: page(),
              total: Math.ceil((logs()?.total ?? 0) / PAGE_LIMIT),
            })}
          </span>
          <Button disabled={!hasNext()} onClick={next} variant="ghost">
            {t('nextPage')}
            <VsChevronRight class="size-4" />
          </Button>
        </div>
      </Show>
    </SectionCard>
  );
};
