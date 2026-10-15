// 批注笔记编辑弹窗：划词记笔记与批注面板编辑共用。

import { createEffect, createSignal } from 'solid-js';

import { Button, Modal } from '../components/ui';
import { useI18n } from '../i18n';

export const NoteEditorModal = (props: {
  busy: boolean;
  initial: string;
  onClose: () => void;
  onSave: (content: string) => void;
  open: boolean;
  title: string;
}) => {
  const { t } = useI18n();
  const [draft, setDraft] = createSignal('');
  // 每次打开时重置为初始内容，关闭后保留草稿无意义
  createEffect(() => {
    if (props.open) setDraft(props.initial);
  });
  return (
    <Modal onClose={props.onClose} open={props.open} title={props.title}>
      <textarea
        class="border-edge bg-surface text-ink focus:border-accent h-28 w-full resize-none rounded border px-3 py-2 text-sm outline-none"
        onInput={event => setDraft(event.currentTarget.value)}
        placeholder={t('readerNotePlaceholder')}
        value={draft()}
      />
      <div class="mt-3 flex justify-end gap-2">
        <Button onClick={props.onClose} variant="ghost">
          {t('cancel')}
        </Button>
        <Button
          disabled={props.busy || !draft().trim()}
          onClick={() => {
            props.onSave(draft().trim());
          }}
        >
          {t('save')}
        </Button>
      </div>
    </Modal>
  );
};
