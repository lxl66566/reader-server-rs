// Generic confirm modal for admin actions.

import type { ParentComponent } from 'solid-js';
import { useI18n } from '../../i18n';
import { Button, Modal } from '../ui';

export const ConfirmDialog: ParentComponent<{
  message: string;
  onClose: () => void;
  onConfirm: () => void;
  open: boolean;
  title: string;
}> = props => {
  const { t } = useI18n();
  return (
    <Modal onClose={props.onClose} open={props.open} title={props.title}>
      <p class="mb-4 text-sm text-muted">{props.message}</p>
      <div class="flex justify-end gap-2">
        <Button onClick={props.onClose} variant="ghost">
          {t('cancel')}
        </Button>
        <Button onClick={props.onConfirm} variant="danger">
          {t('confirm')}
        </Button>
      </div>
    </Modal>
  );
};
