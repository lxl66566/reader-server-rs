// Edit modal for the accumulated reading time of a book (h / m / s inputs).
import { Show, createEffect, createSignal, type Component } from 'solid-js';
import toast from 'solid-toast';

import { useI18n } from '../../i18n';
import { Button, Input, Modal } from '../ui';

// 与服务端 MAX_READING_TIME_SECS 一致：3650 天
const MAX_SECONDS = 3650 * 24 * 60 * 60;

export const EditReadingTimeModal: Component<{
  onClose: () => void;
  onSave: (seconds: number) => Promise<void>;
  open: boolean;
  seconds: number;
}> = props => {
  const { t } = useI18n();
  const [hours, setHours] = createSignal('0');
  const [minutes, setMinutes] = createSignal('0');
  const [seconds, setSeconds] = createSignal('0');
  const [busy, setBusy] = createSignal(false);

  // 打开时按服务端现值回填三段输入
  createEffect(() => {
    if (props.open) {
      setHours(String(Math.floor(props.seconds / 3600)));
      setMinutes(String(Math.floor((props.seconds % 3600) / 60)));
      setSeconds(String(props.seconds % 60));
    }
  });

  // 三段均为非负整数且合计不超上限才允许保存；NaN / 超限返回 null
  const totalSeconds = () => {
    const h = Number(hours());
    const m = Number(minutes());
    const s = Number(seconds());
    if (![h, m, s].every(n => Number.isInteger(n) && n >= 0)) return null;
    const total = h * 3600 + m * 60 + s;
    return total <= MAX_SECONDS ? total : null;
  };

  const save = async () => {
    const total = totalSeconds();
    if (total === null) return;
    setBusy(true);
    try {
      await props.onSave(total);
      toast.success(t('editReadingTimeSuccess'));
      props.onClose();
    } catch {
      toast.error(t('errorGeneric'));
    } finally {
      setBusy(false);
    }
  };

  const onEnter = (event: KeyboardEvent) => {
    if (event.key === 'Enter') void save();
  };

  const field = (
    label: string,
    value: () => string,
    setValue: (v: string) => void,
  ) => (
    <label class="min-w-0 flex-1">
      <span class="text-muted mb-1 block text-center text-xs">{label}</span>
      <Input
        onInput={setValue}
        onKeyDown={onEnter}
        type="number"
        value={value()}
      />
    </label>
  );

  return (
    <Modal
      onClose={props.onClose}
      open={props.open}
      title={t('editReadingTime')}
    >
      <div class="space-y-4">
        <div class="flex gap-2">
          {field(t('durationHours'), hours, setHours)}
          {field(t('durationMinutes'), minutes, setMinutes)}
          {field(t('durationSeconds'), seconds, setSeconds)}
        </div>
        <Show when={totalSeconds() === null}>
          <p class="text-muted text-xs">{t('editReadingTimeInvalid')}</p>
        </Show>
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
            disabled={busy() || totalSeconds() === null}
            onClick={() => void save()}
            type="button"
          >
            {t('save')}
          </Button>
        </div>
      </div>
    </Modal>
  );
};
