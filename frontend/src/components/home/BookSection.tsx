// Section wrapper that owns loading / error / empty rendering for book grids.

import { Show, type ParentComponent } from 'solid-js';

import { useI18n } from '../../i18n';
import { Button, EmptyState, SectionCard, Spinner } from '../ui';

export const BookSection: ParentComponent<{
  actions?: import('solid-js').JSX.Element;
  empty: boolean;
  emptyText: string;
  error: boolean;
  hasData: boolean;
  loading: boolean;
  onRetry: () => void;
  title: string;
}> = props => {
  const { t } = useI18n();
  return (
    <SectionCard actions={props.actions} title={props.title}>
      <Show
        fallback={
          <div class="text-muted flex flex-col items-center gap-3 py-10 text-sm">
            <span>{t('errorLoadFailed')}</span>
            <Button
              onClick={() => {
                props.onRetry();
              }}
              variant="ghost"
            >
              {t('retry')}
            </Button>
          </div>
        }
        when={!props.error}
      >
        <Show
          fallback={
            <div class="flex justify-center py-10">
              <Spinner />
            </div>
          }
          when={!props.loading || props.hasData}
        >
          <Show
            fallback={<EmptyState text={props.emptyText} />}
            when={!props.empty}
          >
            {props.children}
          </Show>
        </Show>
      </Show>
    </SectionCard>
  );
};
