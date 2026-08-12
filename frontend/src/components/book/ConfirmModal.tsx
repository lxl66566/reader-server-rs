// Reusable destructive-action confirmation modal with a busy state.
import { VsWarning } from 'solid-icons/vs';
import { createSignal, type Component } from 'solid-js';
import toast from 'solid-toast';
import { useI18n } from '../../i18n';
import { Button, Modal } from '../ui';

export const ConfirmModal: Component<{
  message: string;
  onClose: () => void;
  onConfirm: () => Promise<void> | void;
  open: boolean;
}> = props => {
  const { t } = useI18n();
  const [busy, setBusy] = createSignal(false);

  const confirm = async () => {
    setBusy(true);
    try {
      await props.onConfirm();
      props.onClose();
    } catch {
      toast.error(t('errorGeneric'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal onClose={props.onClose} open={props.open} title={t('confirmAction')}>
      <p class="mb-4 flex items-start gap-2 text-sm text-ink">
        <VsWarning class="mt-0.5 size-4 shrink-0 text-accent" />
        {props.message}
      </p>
      <div class="flex justify-end gap-2">
        <Button
          disabled={busy()}
          onClick={props.onClose}
          type="button"
          variant="ghost"
        >
          {t('cancel')}
        </Button>
        <Button
          disabled={busy()}
          onClick={() => void confirm()}
          type="button"
          variant="danger"
        >
          {t('delete')}
        </Button>
      </div>
    </Modal>
  );
};
