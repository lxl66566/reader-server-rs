// Prev / next pager for paged book sections.

import { VsChevronLeft, VsChevronRight } from 'solid-icons/vs';
import type { ParentComponent } from 'solid-js';
import { useI18n } from '../../i18n';
import { Button } from '../ui';

export const Pagination: ParentComponent<{
  disabled?: boolean;
  onChange: (page: number) => void;
  page: number;
  totalPages: number;
}> = props => {
  const { t } = useI18n();
  return (
    <div class="mt-4 flex items-center justify-center gap-3 text-sm">
      <Button
        class="px-2"
        disabled={(props.disabled ?? false) || props.page <= 1}
        onClick={() => {
          props.onChange(props.page - 1);
        }}
        variant="ghost"
      >
        <VsChevronLeft class="size-4" />
        {t('prevPage')}
      </Button>
      <span class="text-muted">
        {t('pageIndicator', { page: props.page, total: props.totalPages })}
      </span>
      <Button
        class="px-2"
        disabled={(props.disabled ?? false) || props.page >= props.totalPages}
        onClick={() => {
          props.onChange(props.page + 1);
        }}
        variant="ghost"
      >
        {t('nextPage')}
        <VsChevronRight class="size-4" />
      </Button>
    </div>
  );
};
