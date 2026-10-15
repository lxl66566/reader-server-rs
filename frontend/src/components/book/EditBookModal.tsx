// Edit modal for book metadata (title / author / description / visibility).
import { createEffect, createSignal, Show, type Component } from 'solid-js';
import toast from 'solid-toast';

import type { BookDetail } from '../../api/types';
import { useI18n } from '../../i18n';
import {
  ChapterPatternField,
  isValidChapterPattern,
} from '../ChapterPatternField';
import { Button, Input, Modal, Textarea, Toggle } from '../ui';

// 与后端 PUT /books/:id 的 description 上限一致
const DESCRIPTION_MAX = 2000;

// Patch shape accepted by bookApi.update.
export interface BookPatch {
  author?: string;
  // 仅 txt：null = 恢复内置默认切分规则
  chapter_pattern?: null | string;
  // 空/纯空白 = 清空简介
  description?: string;
  is_public?: boolean;
  title?: string;
}

export const EditBookModal: Component<{
  book: BookDetail;
  onClose: () => void;
  onSave: (patch: BookPatch) => Promise<void>;
  open: boolean;
}> = props => {
  const { t } = useI18n();
  const [title, setTitle] = createSignal('');
  const [author, setAuthor] = createSignal('');
  const [chapterPattern, setChapterPattern] = createSignal('');
  const [description, setDescription] = createSignal('');
  const [isPublic, setIsPublic] = createSignal(false);
  const [busy, setBusy] = createSignal(false);

  const isTxt = () => props.book.format === 'txt';

  // Reset the form to the server value each time the modal opens.
  createEffect(() => {
    if (props.open) {
      setTitle(props.book.title);
      setAuthor(props.book.author ?? '');
      setChapterPattern(props.book.chapter_pattern ?? '');
      setDescription(props.book.description ?? '');
      setIsPublic(props.book.is_public);
    }
  });

  const save = async () => {
    if (!title().trim()) return;
    setBusy(true);
    const patch: BookPatch = {
      author: author().trim() || undefined,
      chapter_pattern: isTxt() ? chapterPattern().trim() || null : undefined,
      // description 恒发送：空串语义为「清空」，缺省才是「不修改」
      description: description().trim(),
      is_public: isPublic(),
      title: title().trim(),
    };
    try {
      await props.onSave(patch);
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

  return (
    <Modal onClose={props.onClose} open={props.open} title={t('editBook')}>
      <div class="space-y-4">
        <label class="block">
          <span class="text-muted mb-1 block text-sm">{t('titleLabel')}</span>
          <Input onInput={setTitle} onKeyDown={onEnter} value={title()} />
        </label>
        <label class="block">
          <span class="text-muted mb-1 block text-sm">{t('authorLabel')}</span>
          <Input onInput={setAuthor} onKeyDown={onEnter} value={author()} />
        </label>
        <label class="block">
          <span class="text-muted mb-1 block text-sm">
            {t('synopsisLabel')}
          </span>
          <Textarea
            maxLength={DESCRIPTION_MAX}
            onInput={setDescription}
            value={description()}
          />
          <span class="text-muted mt-1 block text-right text-xs">
            {description().length} / {DESCRIPTION_MAX}
          </span>
        </label>
        <Show when={isTxt()}>
          <ChapterPatternField
            onInput={setChapterPattern}
            value={chapterPattern()}
          />
        </Show>
        <Toggle
          checked={isPublic()}
          label={t('isPublicLabel')}
          onChange={setIsPublic}
        />
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
            disabled={
              busy() ||
              !title().trim() ||
              (isTxt() && !isValidChapterPattern(chapterPattern()))
            }
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
