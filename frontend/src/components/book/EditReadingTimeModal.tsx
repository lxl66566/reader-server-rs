// Edit modal for the accumulated reading time of a book (h / m / s inputs).
// 手动编辑只改累计时长，不影响每日统计；提示并提供跳转统计页编辑每日
// 时长的深链（?editBook=<id>，由统计页自动打开对应书的编辑弹窗）。
import { useNavigate } from '@solidjs/router';
import { Show, createEffect, createSignal, type Component } from 'solid-js';
import toast from 'solid-toast';

import { useI18n } from '../../i18n';
import { Button, Input, Modal } from '../ui';

// 与服务端 MAX_READING_TIME_SECS 一致：3650 天
const MAX_SECONDS = 3650 * 24 * 60 * 60;

export const EditReadingTimeModal: Component<{
  /** 用于跳转统计页编辑每日时长的深链 */
  bookId: number;
  onClose: () => void;
  onSave: (seconds: number) => Promise<void>;
  open: boolean;
  seconds: number;
}> = props => {
  const { t } = useI18n();
  const navigate = useNavigate();
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

  // 跳统计页编辑每日时长（关闭本弹窗后导航）
  const goToDailyEdit = () => {
    props.onClose();
    navigate(`/stats?editBook=${props.bookId}`);
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
        <p class="text-muted text-xs">
          {t('editReadingTimeDailyHint')}
          <button
            class="text-accent ml-1 cursor-pointer underline underline-offset-2"
            onClick={goToDailyEdit}
            type="button"
          >
            {t('goToStatsEditDaily')}
          </button>
        </p>
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
